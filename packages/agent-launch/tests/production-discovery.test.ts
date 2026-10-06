import { access, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  ControlPlaneError,
  SecretToken,
  createDiscoveryDescriptor,
  deriveScope,
} from "@lyonbrown/local-control-plane";
import { createNativeControlPlaneProvider } from "@lyonbrown/local-control-plane/native-adapter";
import { afterEach, describe, expect, test } from "vitest";
import {
  createProductionAgentClientFactory,
  createProductionDiscoveryClient,
} from "@lyonbrown/agent-launch/internal/production-discovery";

const WORKSPACE = process.platform === "win32" ? "C:\\workspace" : "/workspace";
const CONFIGURATION = "production-discovery-test";
const SERVER_ID = "e6b0f57b-bc22-4bb9-94df-91cc78aaad73";

interface DirectoryState {
  descriptor: Uint8Array | null;
  lockHeld: boolean;
  closeCalls: number;
  releaseCalls: number;
  afterAcquire?: () => void;
  releaseBarrier?: Promise<void>;
  pathExistedBeforeNativeOpen?: boolean;
  workspaceFailuresRemaining?: number;
}

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true })),
  );
});

async function temporaryHome(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "lyonbrown-agent-launch-"));
  temporaryDirectories.push(directory);
  return directory;
}

function secureFixture(state: DirectoryState) {
  const handle = {
    readPrivateFile: (_name: "agentd.json", maximumBytes: number) => {
      if (state.descriptor !== null && state.descriptor.byteLength > maximumBytes) {
        return Promise.resolve({ ok: false as const, code: "oversize" as const });
      }
      return Promise.resolve({ ok: true as const, value: state.descriptor });
    },
    atomicReplacePrivateFile: () =>
      Promise.resolve({ ok: false as const, code: "access_denied" as const }),
    tryAcquireKernelLock: (_name: "agentd.start.lock" | "agentd.lock") => {
      void _name;
      if (state.lockHeld) return Promise.resolve({ ok: true as const, value: null });
      state.lockHeld = true;
      state.afterAcquire?.();
      return Promise.resolve({
        ok: true as const,
        value: {
          release: async () => {
            state.releaseCalls += 1;
            await state.releaseBarrier;
            state.lockHeld = false;
            return { ok: true as const, value: undefined };
          },
        },
      });
    },
    close: () => {
      state.closeCalls += 1;
      return Promise.resolve({ ok: true as const, value: undefined });
    },
  };
  const provider = createNativeControlPlaneProvider({
    contract:
      process.platform === "win32"
        ? {
            platform: "windows",
            directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT",
            ownerProtection: "current-user-only-dacl",
            workspaceIdentity: "native-file-id-v1",
            kernelLock: "kernel-file-lock-v1",
          }
        : {
            platform: "posix",
            directoryOpen: "dirfd-openat-O_NOFOLLOW",
            ownerProtection: "current-uid-owner-mode",
            workspaceIdentity: "native-dev-ino-v1",
            kernelLock: "kernel-file-lock-v1",
          },
    workspaceIdentity: (path) => {
      if ((state.workspaceFailuresRemaining ?? 0) > 0) {
        state.workspaceFailuresRemaining = (state.workspaceFailuresRemaining ?? 0) - 1;
        return Promise.resolve({ ok: false as const, code: "io" as const });
      }
      return Promise.resolve({
        ok: true,
        value: {
          canonicalPath: path,
          identity: { kind: "inode", deviceId: 3n, inodeNumber: 7n },
        },
      });
    },
    openSecureRunDirectory: async (path) => {
      state.pathExistedBeforeNativeOpen = await access(path).then(
        () => true,
        () => false,
      );
      return { ok: true as const, value: handle };
    },
  });
  return { provider };
}

async function contextFor(provider: ReturnType<typeof secureFixture>["provider"]) {
  return {
    nativeProvider: provider,
    scope: await deriveScope(WORKSPACE, CONFIGURATION, provider),
  };
}

describe("production discovery adapter", () => {
  test("reads an opaque authenticated capability without exposing transport credentials", async () => {
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
    };
    const { provider } = secureFixture(state);
    const context = await contextFor(provider);
    const descriptor = createDiscoveryDescriptor({
      endpoint: "127.0.0.1:43121",
      pid: 321,
      scopeId: context.scope.scopeId,
      serverInstanceId: SERVER_ID,
      token: SecretToken.parse("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    });
    state.descriptor = Buffer.from(
      JSON.stringify({
        schema_version: descriptor.schemaVersion,
        transport: descriptor.transport,
        address: descriptor.endpoint,
        pid: descriptor.pid,
        server_instance_id: descriptor.serverInstanceId,
        token: descriptor.token.exposeForTransport(),
        scope_fingerprint: descriptor.scopeId,
      }),
    );
    const discovery = createProductionDiscoveryClient({
      nativeProvider: provider,
      homeDirectory: await temporaryHome(),
      effectiveConfigFingerprint: CONFIGURATION,
    });

    const candidate = await discovery.read(context, new AbortController().signal);

    expect(candidate).toEqual({ serverInstanceId: SERVER_ID, scopeId: context.scope.scopeId });
    expect(JSON.stringify(candidate)).not.toContain("43121");
    expect(JSON.stringify(candidate)).not.toContain("AAAA");
    expect(state.closeCalls).toBe(1);

    const factory = createProductionAgentClientFactory();
    if (candidate === null) throw new Error("expected a discovered daemon");
    const client = factory.create(candidate);
    await expect(client.close()).resolves.toBeUndefined();
  });

  test("maps an absent descriptor to no candidate and closes its secure handle", async () => {
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
    };
    const { provider } = secureFixture(state);
    const discovery = createProductionDiscoveryClient({
      nativeProvider: provider,
      homeDirectory: await temporaryHome(),
      effectiveConfigFingerprint: CONFIGURATION,
    });

    await expect(
      discovery.read(await contextFor(provider), new AbortController().signal),
    ).resolves.toBeNull();
    expect(state.closeCalls).toBe(1);
  });

  test("delegates fresh run-directory creation to the native provider", async () => {
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
    };
    const { provider } = secureFixture(state);
    const homeDirectory = await temporaryHome();
    const discovery = createProductionDiscoveryClient({
      nativeProvider: provider,
      homeDirectory,
      effectiveConfigFingerprint: CONFIGURATION,
    });

    await discovery.read(await contextFor(provider), new AbortController().signal);

    expect(state.pathExistedBeforeNativeOpen).toBe(false);
  });

  test("retries path derivation after a transient native identity failure", async () => {
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
    };
    const { provider } = secureFixture(state);
    const context = await contextFor(provider);
    const discovery = createProductionDiscoveryClient({
      nativeProvider: provider,
      homeDirectory: await temporaryHome(),
      effectiveConfigFingerprint: CONFIGURATION,
    });
    state.workspaceFailuresRemaining = 1;

    await expect(discovery.read(context, new AbortController().signal)).rejects.toBeInstanceOf(
      ControlPlaneError,
    );
    await expect(discovery.read(context, new AbortController().signal)).resolves.toBeNull();
  });

  test("holds and idempotently releases the native startup lease", async () => {
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
    };
    const { provider } = secureFixture(state);
    const discovery = createProductionDiscoveryClient({
      nativeProvider: provider,
      homeDirectory: await temporaryHome(),
      effectiveConfigFingerprint: CONFIGURATION,
    });
    const context = await contextFor(provider);
    const signal = new AbortController().signal;

    const lease = await discovery.tryAcquireStartupLease(context, signal);
    expect(lease).not.toBeNull();
    expect(state.lockHeld).toBe(true);
    expect(await discovery.tryAcquireStartupLease(context, signal)).toBeNull();

    if (lease === null) throw new Error("expected a startup lease");
    await Promise.all([lease.release(signal), lease.release(signal)]);
    expect(state.releaseCalls).toBe(1);
    expect(state.lockHeld).toBe(false);
    expect(state.closeCalls).toBe(2);
  });

  test("rejects a context carrying a different native capability", async () => {
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
    };
    const first = secureFixture(state);
    const second = secureFixture(state);
    const discovery = createProductionDiscoveryClient({
      nativeProvider: first.provider,
      homeDirectory: await temporaryHome(),
      effectiveConfigFingerprint: CONFIGURATION,
    });
    const context = await contextFor(second.provider);

    await expect(discovery.read(context, new AbortController().signal)).rejects.toBeInstanceOf(
      ControlPlaneError,
    );
  });

  test("releases a startup lock acquired concurrently with cancellation", async () => {
    const controller = new AbortController();
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
      afterAcquire: () => {
        controller.abort();
      },
    };
    const { provider } = secureFixture(state);
    const discovery = createProductionDiscoveryClient({
      nativeProvider: provider,
      homeDirectory: await temporaryHome(),
      effectiveConfigFingerprint: CONFIGURATION,
    });

    await expect(
      discovery.tryAcquireStartupLease(await contextFor(provider), controller.signal),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(state.releaseCalls).toBe(1);
    expect(state.lockHeld).toBe(false);
    expect(state.closeCalls).toBe(1);
  });

  test("lets an aborted waiter detach while the shared native release completes", async () => {
    let finishRelease!: () => void;
    const releaseBarrier = new Promise<void>((resolve) => {
      finishRelease = resolve;
    });
    const state: DirectoryState = {
      descriptor: null,
      lockHeld: false,
      closeCalls: 0,
      releaseCalls: 0,
      releaseBarrier,
    };
    const { provider } = secureFixture(state);
    const discovery = createProductionDiscoveryClient({
      nativeProvider: provider,
      homeDirectory: await temporaryHome(),
      effectiveConfigFingerprint: CONFIGURATION,
    });
    const lease = await discovery.tryAcquireStartupLease(
      await contextFor(provider),
      new AbortController().signal,
    );
    if (lease === null) throw new Error("expected a startup lease");
    const firstController = new AbortController();

    const firstWaiter = lease.release(firstController.signal);
    firstController.abort();

    await expect(firstWaiter).rejects.toMatchObject({ name: "AbortError" });
    expect(state.releaseCalls).toBe(1);
    expect(state.lockHeld).toBe(true);
    expect(state.closeCalls).toBe(0);

    const finalWaiter = lease.release(new AbortController().signal);
    expect(state.releaseCalls).toBe(1);
    finishRelease();

    await expect(finalWaiter).resolves.toBeUndefined();
    expect(state.releaseCalls).toBe(1);
    expect(state.lockHeld).toBe(false);
    expect(state.closeCalls).toBe(1);

    const alreadyAborted = new AbortController();
    alreadyAborted.abort();
    await expect(lease.release(alreadyAborted.signal)).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(state.releaseCalls).toBe(1);
    expect(state.closeCalls).toBe(1);
  });
});
