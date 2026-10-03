import { describe, expect, it } from "vitest";
import {
  ControlPlaneError,
  acquireLifecycleLock,
  acquireStartupLock,
  createDiscoveryDescriptor,
  openSecureRunDirectory,
  type PlatformSecurityContract,
} from "../src/index.js";
import { FakePlatformSecurity } from "./test-support.js";

const RUN_PATH = "/private/agent/scope";
const SCOPE = "a".repeat(64);

describe("native platform security boundary", () => {
  it("makes the Windows and POSIX security guarantees explicit", () => {
    const contracts: readonly PlatformSecurityContract[] = [
      {
        platform: "windows",
        directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT",
        ownerProtection: "current-user-only-dacl",
        workspaceIdentity: "rust-file-id",
        kernelLock: "rust-compatible-file-lock",
      },
      {
        platform: "posix",
        directoryOpen: "dirfd-openat-O_NOFOLLOW",
        ownerProtection: "current-uid-owner-mode",
        workspaceIdentity: "rust-dev-ino",
        kernelLock: "rust-compatible-file-lock",
      },
    ];
    expect(contracts.map(({ platform }) => platform)).toEqual(["windows", "posix"]);
  });

  it("fails closed when no native provider is supplied", async () => {
    await expect(openSecureRunDirectory(RUN_PATH)).rejects.toMatchObject({
      code: "NATIVE_SECURITY_PROVIDER_REQUIRED",
    });
  });

  it("maps provider failures without exposing dependency causes", async () => {
    const security = new FakePlatformSecurity();
    security.nextFailure = { ok: false, code: "reparse_point" };
    const error = await openSecureRunDirectory(RUN_PATH, security).catch(
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
    const error = await openSecureRunDirectory(RUN_PATH, security).catch(
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
    const directory = await openSecureRunDirectory(RUN_PATH, security);
    const store = directory.discovery;
    const descriptor = createDiscoveryDescriptor({
      endpoint: "127.0.0.1:4321",
      pid: 42,
      scopeId: SCOPE,
    });
    security.replaceDirectoryPath(RUN_PATH);
    await store.publish(descriptor);
    expect((await store.read()).serverInstanceId).toBe(descriptor.serverInstanceId);

    const replacement = (await openSecureRunDirectory(RUN_PATH, security)).discovery;
    await expect(replacement.read()).rejects.toMatchObject({
      code: "DESCRIPTOR_NOT_FOUND",
    });
  });

  it("requires unique same-directory temporaries and cleanup for atomic publication", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security);
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
    const firstHandle = await openSecureRunDirectory(RUN_PATH, security);
    const secondHandle = await openSecureRunDirectory(RUN_PATH, security);
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
      Array.from({ length: 16 }, () => openSecureRunDirectory(RUN_PATH, security)),
    );
    const contenders = await Promise.all(handles.map((handle) => acquireStartupLock(handle)));
    const winners = contenders.filter((lease) => lease !== null);
    expect(winners).toHaveLength(1);
    await winners[0]?.release();
  });

  it("does not let an old owner release a newer kernel lock", async () => {
    const security = new FakePlatformSecurity();
    const handle = await openSecureRunDirectory(RUN_PATH, security);
    const oldOwner = await acquireLifecycleLock(handle);
    await oldOwner?.release();
    const newOwner = await acquireLifecycleLock(handle);
    await oldOwner?.release();
    expect(await acquireLifecycleLock(handle)).toBeNull();
    await newOwner?.release();
  });

  it("redacts native lock release exceptions", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security);
    const lease = await acquireLifecycleLock(directory);
    security.handleException = "release";
    await expect(lease?.release()).rejects.toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
      details: { operation: "release_lock" },
    });
  });

  it("maps oversize secure reads before parsing", async () => {
    const security = new FakePlatformSecurity();
    const directory = await openSecureRunDirectory(RUN_PATH, security);
    const write = await directory.atomicReplacePrivateFile("agentd.json", new Uint8Array(5000));
    expect(write.ok).toBe(true);
    await expect(directory.discovery.read()).rejects.toMatchObject({
      code: "DESCRIPTOR_TOO_LARGE",
    });
  });

  it.each(["atomic", "read", "lock"] as const)(
    "redacts native %s exceptions behind the secure handle",
    async (operation) => {
      const security = new FakePlatformSecurity();
      const directory = await openSecureRunDirectory(RUN_PATH, security);
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
    const directory = await openSecureRunDirectory(RUN_PATH, security);
    expect(await directory.close()).toEqual({ ok: true, value: undefined });
    security.handleException = "close";
    expect(await directory.close()).toEqual({ ok: false, code: "io" });
  });
});
