import { inspect } from "node:util";
import { describe, expect, it } from "vitest";
import {
  ControlPlaneError,
  acquireLifecycleLock,
  acquireStartupLock,
  createDiscoveryDescriptor,
  openSecureRunDirectory,
  type NativeControlPlaneProvider,
  type SecureRunDirectory,
} from "../src/index.ts";
import type { PlatformSecurityContract, SecureDirectoryHandle } from "../src/native-adapter.ts";
import { FakePlatformSecurity } from "./test-support.ts";

const RUN_PATH = "/private/agent/scope";
const SCOPE = "a".repeat(64);
type RuntimeConstructor = new (...arguments_: readonly unknown[]) => unknown;

function getRuntimeConstructor(value: object): RuntimeConstructor {
  const prototype: unknown = Object.getPrototypeOf(value);
  if (typeof prototype !== "object" || prototype === null) throw new Error("missing prototype");
  const constructor: unknown = Reflect.get(prototype, "constructor");
  if (typeof constructor !== "function") throw new Error("missing constructor");
  return constructor as RuntimeConstructor;
}

describe("native platform security boundary", () => {
  it("makes the Windows and POSIX security guarantees explicit", () => {
    const contracts: readonly PlatformSecurityContract[] = [
      {
        platform: "windows",
        directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT",
        ownerProtection: "current-user-only-dacl",
        workspaceIdentity: "native-file-id-v1",
        kernelLock: "kernel-file-lock-v1",
      },
      {
        platform: "posix",
        directoryOpen: "dirfd-openat-O_NOFOLLOW",
        ownerProtection: "current-uid-owner-mode",
        workspaceIdentity: "native-dev-ino-v1",
        kernelLock: "kernel-file-lock-v1",
      },
    ];
    expect(contracts.map(({ platform }) => platform)).toEqual(["windows", "posix"]);
  });

  it("fails closed when no native provider is supplied", async () => {
    await expect(openSecureRunDirectory(RUN_PATH)).rejects.toMatchObject({
      code: "NATIVE_SECURITY_PROVIDER_REQUIRED",
    });
  });

  it("rejects forged provider and run-directory capabilities", async () => {
    await expect(
      openSecureRunDirectory(RUN_PATH, {} as NativeControlPlaneProvider),
    ).rejects.toMatchObject({ code: "NATIVE_SECURITY_PROVIDER_REQUIRED" });
    await expect(acquireStartupLock({} as SecureRunDirectory)).rejects.toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
    });
  });

  it("maps provider failures without exposing dependency causes", async () => {
    const security = new FakePlatformSecurity();
    security.nextFailure = { ok: false, code: "reparse_point" };
    const error = await openSecureRunDirectory(RUN_PATH, security.provider).catch(
      (caught: unknown) => caught,
    );
    expect(error).toBeInstanceOf(ControlPlaneError);
    expect(error).toMatchObject({ code: "PLATFORM_SECURITY_FAILURE" });
    expect(error).not.toHaveProperty("cause");
    expect(JSON.stringify(error)).not.toContain(RUN_PATH);
  });

  it("redacts an unexpected native exception", async () => {
    const security = new FakePlatformSecurity();
    security.openSecureRunDirectory = () => {
      throw new Error("secret native path and token");
    };
    const error = await openSecureRunDirectory(RUN_PATH, security.provider).catch(
      (caught: unknown) => caught,
    );
    expect(error).toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
      details: { operation: "open_run_directory" },
    });
    expect(JSON.stringify(error)).not.toContain("secret native path and token");
  });

  it("keeps discovery anchored when the parent path is swapped", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
    const store = directory.discovery;
    expect(store.fileName()).toBe("agentd.json");
    const descriptor = createDiscoveryDescriptor({
      endpoint: "127.0.0.1:4321",
      pid: 42,
      scopeId: SCOPE,
    });
    security.replaceDirectoryPath(RUN_PATH);
    await store.publish(descriptor);
    expect((await store.read()).serverInstanceId).toBe(descriptor.serverInstanceId);

    const replacement = (await openSecureRunDirectory(RUN_PATH, security.provider)).discovery;
    await expect(replacement.read()).rejects.toMatchObject({
      code: "DESCRIPTOR_NOT_FOUND",
    });
  });

  it("does not expose native capabilities through runtime object state", async () => {
    const security = new FakePlatformSecurity();
    const provider = security.provider;
    const directory = await openSecureRunDirectory(RUN_PATH, provider);
    const lease = await acquireLifecycleLock(directory);

    expect(Reflect.ownKeys(provider)).toEqual([]);
    expect(Object.getOwnPropertyNames(directory)).toEqual(["discovery"]);
    expect(Reflect.ownKeys(directory.discovery)).toEqual([]);
    expect(Reflect.ownKeys(lease ?? {})).toEqual(["kind", "release"]);
    expect("readPrivateFile" in directory).toBe(false);
    expect("atomicReplacePrivateFile" in directory).toBe(false);
    expect("tryAcquireKernelLock" in directory).toBe(false);
    expect("readPrivateFile" in directory.discovery).toBe(false);
    expect("atomicReplacePrivateFile" in directory.discovery).toBe(false);
    expect("tryAcquireKernelLock" in directory.discovery).toBe(false);
    expect(Object.getOwnPropertyNames(Object.getPrototypeOf(directory) as object)).toEqual([
      "constructor",
      "close",
    ]);
    const detachedRead = directory.discovery.read.bind({});
    await expect(detachedRead()).rejects.toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
      details: { operation: "read_descriptor" },
    });

    const clone = structuredClone({ provider, directory });
    expect(Reflect.ownKeys(clone.provider)).toEqual([]);
    expect(Reflect.ownKeys(clone.directory)).toEqual(["discovery"]);
    expect(Reflect.ownKeys(clone.directory.discovery as object)).toEqual([]);

    const observable = `${JSON.stringify({ provider, directory, lease })}\n${inspect({
      provider,
      directory,
      lease,
    })}`;
    expect(observable).not.toMatch(
      /nativeHandle|readPrivateFile|atomicReplacePrivateFile|tryAcquireKernelLock/u,
    );
    await lease?.release();
  });

  it("authenticates opaque constructors before touching an attacker handle", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
    let fakeCalls = 0;
    const attackerHandle: SecureDirectoryHandle = {
      readPrivateFile: () => {
        fakeCalls += 1;
        return Promise.resolve({ ok: true, value: null });
      },
      atomicReplacePrivateFile: () => {
        fakeCalls += 1;
        return Promise.resolve({ ok: true, value: undefined });
      },
      tryAcquireKernelLock: () => {
        fakeCalls += 1;
        return Promise.resolve({ ok: true, value: null });
      },
      close: () => {
        fakeCalls += 1;
        return Promise.resolve({ ok: true, value: undefined });
      },
    };
    const directoryConstructor = getRuntimeConstructor(directory);
    const discoveryConstructor = getRuntimeConstructor(directory.discovery);

    for (const constructor of [directoryConstructor, discoveryConstructor]) {
      expect(() => new constructor()).toThrow();
      expect(() => new constructor({}, attackerHandle)).toThrow();
      expect(() => new constructor(attackerHandle)).toThrow();
    }
    expect(fakeCalls).toBe(0);
  });

  it("requires unique same-directory temporaries and cleanup for atomic publication", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
    const descriptor = createDiscoveryDescriptor({
      endpoint: "[::1]:4321",
      pid: 42,
      scopeId: SCOPE,
    });
    await directory.discovery.publish(descriptor);
    await directory.discovery.publish(descriptor);
    expect(new Set(security.temporaryNames)).toHaveLength(2);
    expect(security.liveTemporaryNames).toHaveLength(0);
  });

  it("uses separate kernel startup and lifetime locks", async () => {
    const security = new FakePlatformSecurity();
    const firstHandle = await openSecureRunDirectory(RUN_PATH, security.provider);
    const secondHandle = await openSecureRunDirectory(RUN_PATH, security.provider);
    const startup = await acquireStartupLock(firstHandle);
    expect(startup).not.toBeNull();
    expect(await acquireStartupLock(secondHandle)).toBeNull();
    const lifetime = await acquireLifecycleLock(secondHandle);
    expect(lifetime).not.toBeNull();
    await startup?.release();
    expect(await acquireStartupLock(secondHandle)).not.toBeNull();
    await lifetime?.release();
  });

  it("elects exactly one concurrent first-start winner", async () => {
    const security = new FakePlatformSecurity();
    const handles = await Promise.all(
      Array.from({ length: 16 }, () => openSecureRunDirectory(RUN_PATH, security.provider)),
    );
    const contenders = await Promise.all(handles.map((handle) => acquireStartupLock(handle)));
    const winners = contenders.filter((lease) => lease !== null);
    expect(winners).toHaveLength(1);
    await winners[0]?.release();
  });

  it("does not let an old owner release a newer kernel lock", async () => {
    const security = new FakePlatformSecurity();
    const handle = await openSecureRunDirectory(RUN_PATH, security.provider);
    const oldOwner = await acquireLifecycleLock(handle);
    await oldOwner?.release();
    const newOwner = await acquireLifecycleLock(handle);
    await oldOwner?.release();
    expect(await acquireLifecycleLock(handle)).toBeNull();
    await newOwner?.release();
  });

  it("redacts native lock release exceptions", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
    const lease = await acquireLifecycleLock(directory);
    security.handleException = "release";
    const first = lease?.release();
    const second = lease?.release();
    expect(first).toBe(second);
    await expect(first).rejects.toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
      details: { operation: "release_lock" },
    });
    await expect(second).rejects.toMatchObject({ code: "PLATFORM_SECURITY_FAILURE" });
    expect(lease?.release()).toBe(first);
    expect(security.operations.filter((operation) => operation === "release")).toHaveLength(1);
  });

  it("shares one native release promise across concurrent successful callers", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
    const lease = await acquireLifecycleLock(directory);
    const first = lease?.release();
    const second = lease?.release();
    expect(first).toBe(second);
    await Promise.all([first, second]);
    expect(security.operations.filter((operation) => operation === "release")).toHaveLength(1);
  });

  it("maps oversize secure reads before parsing", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
    security.writeRawDescriptor(RUN_PATH, new Uint8Array(5000));
    await expect(directory.discovery.read()).rejects.toMatchObject({
      code: "DESCRIPTOR_TOO_LARGE",
    });
  });

  it.each(["atomic", "read", "lock"] as const)(
    "redacts native %s exceptions behind the secure handle",
    async (operation) => {
      const security = new FakePlatformSecurity();
      const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
      security.handleException = operation;
      const action =
        operation === "atomic"
          ? directory.discovery.publish(
              createDiscoveryDescriptor({ endpoint: "[::1]:4321", pid: 1, scopeId: SCOPE }),
            )
          : operation === "read"
            ? directory.discovery.read()
            : acquireStartupLock(directory);
      await expect(action).rejects.toMatchObject({ code: "PLATFORM_SECURITY_FAILURE" });
    },
  );

  it("closes through the native handle without leaking close failures", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security.provider);
    await expect(directory.close()).resolves.toBeUndefined();
    security.handleException = "close";
    await expect(directory.close()).rejects.toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
      details: { operation: "close_directory" },
    });
  });
});
