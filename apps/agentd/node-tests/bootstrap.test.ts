import {
  deriveScopedDaemonPaths,
  parseDiscoveryDescriptor,
  type ControlPlaneError,
} from "@lyonbrown/local-control-plane";
import {
  createNativeControlPlaneProvider,
  type KernelFileLock,
  type PlatformControlPlaneSecurity,
  type PlatformResult,
  type SecureDirectoryHandle,
} from "@lyonbrown/local-control-plane/native-adapter";
import { describe, expect, it, vi } from "vitest";

import {
  startProductionAgentd,
  type AgentdBootstrapError,
  type ProductionAgentdRuntime,
} from "@lyonbrown/agentd/internal/bootstrap";
import type { StartAgentdServerOptions } from "@lyonbrown/agentd/internal/server";
import { FakeHost } from "@lyonbrown/agentd/test/server-test-support";

const WORKSPACE = "C:\\work\\lyonbrown";
const HOME_DIRECTORY = "C:\\Users\\agent";
const FINGERPRINT = "production-test";

interface NativeHarness {
  readonly events: string[];
  readonly provider: ReturnType<typeof createNativeControlPlaneProvider>;
  descriptorBytes(): Uint8Array | undefined;
}

function successful<T>(value: T): PlatformResult<T> {
  return { ok: true, value };
}

function nativeHarness(
  options: { readonly lockAvailable?: boolean; readonly publishFails?: boolean } = {},
): NativeHarness {
  const events: string[] = [];
  let descriptor: Uint8Array | undefined;
  const lock: KernelFileLock = {
    release: async () => {
      events.push("lease.release");
      return successful(undefined);
    },
  };
  const directory: SecureDirectoryHandle = {
    readPrivateFile: async () => successful(descriptor ?? null),
    atomicReplacePrivateFile: async (_name, bytes) => {
      events.push("descriptor.publish");
      if (options.publishFails === true) return { ok: false, code: "io" };
      descriptor = Uint8Array.from(bytes);
      return successful(undefined);
    },
    tryAcquireKernelLock: async () => {
      events.push("lease.acquire");
      return successful(options.lockAvailable === false ? null : lock);
    },
    close: async () => {
      events.push("directory.close");
      return successful(undefined);
    },
  };
  const security: PlatformControlPlaneSecurity = {
    contract: {
      platform: "windows",
      directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT",
      ownerProtection: "current-user-only-dacl",
      workspaceIdentity: "native-file-id-v1",
      kernelLock: "kernel-file-lock-v1",
    },
    workspaceIdentity: async () => {
      events.push("workspace.identity");
      return successful({
        canonicalPath: WORKSPACE,
        identity: { kind: "windows-high-res", volumeSerialNumber: 7n, fileId: 11n },
      });
    },
    openSecureRunDirectory: async (path) => {
      events.push(`directory.open:${path}`);
      return successful(directory);
    },
  };
  return {
    events,
    provider: createNativeControlPlaneProvider(security),
    descriptorBytes: () => descriptor,
  };
}

function runtime(events: string[]): ProductionAgentdRuntime {
  return {
    host: new FakeHost(),
    close: vi.fn(async () => {
      events.push("runtime.close");
    }),
  };
}

async function expectedScope(native: NativeHarness): Promise<string> {
  const paths = await deriveScopedDaemonPaths(
    HOME_DIRECTORY,
    WORKSPACE,
    FINGERPRINT,
    native.provider,
  );
  native.events.splice(0);
  return paths.scopeId;
}

describe("production bootstrap", () => {
  it("fails closed before runtime composition when no native provider exists", async () => {
    await expect(
      startProductionAgentd({
        workspace: process.cwd(),
        homeDirectory: process.cwd(),
        effectiveConfigFingerprint: "test",
        expectedScopeId: "a".repeat(64),
        createRuntime: async () => runtime([]),
      }),
    ).rejects.toMatchObject({
      code: "NATIVE_SECURITY_PROVIDER_REQUIRED",
    } satisfies Partial<ControlPlaneError>);
  });

  it("publishes the authenticated bound server descriptor and closes owned resources in reverse order", async () => {
    const native = nativeHarness();
    const expectedScopeId = await expectedScope(native);
    let serverOptions: StartAgentdServerOptions | undefined;
    const running = await startProductionAgentd(
      {
        workspace: WORKSPACE,
        homeDirectory: HOME_DIRECTORY,
        effectiveConfigFingerprint: FINGERPRINT,
        expectedScopeId,
        nativeProvider: native.provider,
        createRuntime: async ({ canonicalWorkspace, dataDirectory, scopeId }) => {
          native.events.push("runtime.create");
          expect(canonicalWorkspace).toBe(WORKSPACE);
          expect(dataDirectory).toMatch(/\\data$/u);
          expect(scopeId).toMatch(/^[a-f0-9]{64}$/u);
          return runtime(native.events);
        },
      },
      {
        startServer: async (options) => {
          serverOptions = options;
          native.events.push("server.start");
          return {
            address: "127.0.0.1:43123",
            port: 43123,
            close: async () => {
              native.events.push("server.close");
            },
          };
        },
      },
    );

    const descriptorBytes = native.descriptorBytes();
    expect(descriptorBytes).toBeDefined();
    if (descriptorBytes === undefined) throw new Error("descriptor was not published");
    const descriptor = parseDiscoveryDescriptor(descriptorBytes);
    expect(descriptor).toMatchObject({
      endpoint: "127.0.0.1:43123",
      pid: process.pid,
      serverInstanceId: serverOptions?.serverInstanceId,
      scopeId: serverOptions?.scopeFingerprint,
    });
    expect(descriptor.token.exposeForTransport()).toBe(serverOptions?.token.exposeForTransport());
    expect(native.events).toEqual([
      "workspace.identity",
      expect.stringMatching(/^directory\.open:/u),
      "lease.acquire",
      "runtime.create",
      "server.start",
      "descriptor.publish",
    ]);

    await running.close();
    await running.close();
    expect(native.events.slice(-4)).toEqual([
      "server.close",
      "runtime.close",
      "lease.release",
      "directory.close",
    ]);
  });

  it("fails when another daemon owns the lifecycle lease and closes the secure directory", async () => {
    const native = nativeHarness({ lockAvailable: false });
    const expectedScopeId = await expectedScope(native);
    const createRuntime = vi.fn(async () => runtime(native.events));

    await expect(
      startProductionAgentd({
        workspace: WORKSPACE,
        homeDirectory: HOME_DIRECTORY,
        effectiveConfigFingerprint: FINGERPRINT,
        expectedScopeId,
        nativeProvider: native.provider,
        createRuntime,
      }),
    ).rejects.toMatchObject({
      code: "DAEMON_ALREADY_RUNNING",
    } satisfies Partial<AgentdBootstrapError>);
    expect(createRuntime).not.toHaveBeenCalled();
    expect(native.events.slice(-2)).toEqual(["lease.acquire", "directory.close"]);
  });

  it("rolls back server, runtime, lease, and directory when descriptor publication fails", async () => {
    const native = nativeHarness({ publishFails: true });
    const expectedScopeId = await expectedScope(native);

    await expect(
      startProductionAgentd(
        {
          workspace: WORKSPACE,
          homeDirectory: HOME_DIRECTORY,
          effectiveConfigFingerprint: FINGERPRINT,
          expectedScopeId,
          nativeProvider: native.provider,
          createRuntime: async () => runtime(native.events),
        },
        {
          startServer: async () => ({
            address: "127.0.0.1:43124",
            port: 43124,
            close: async () => {
              native.events.push("server.close");
            },
          }),
        },
      ),
    ).rejects.toMatchObject({ code: "PLATFORM_SECURITY_FAILURE" });
    expect(native.events.slice(-5)).toEqual([
      "descriptor.publish",
      "server.close",
      "runtime.close",
      "lease.release",
      "directory.close",
    ]);
  });

  it("fails closed before opening the run directory when launcher scope does not match", async () => {
    const native = nativeHarness();
    const createRuntime = vi.fn(async () => runtime(native.events));

    await expect(
      startProductionAgentd({
        workspace: WORKSPACE,
        homeDirectory: HOME_DIRECTORY,
        effectiveConfigFingerprint: FINGERPRINT,
        expectedScopeId: "0".repeat(64),
        nativeProvider: native.provider,
        createRuntime,
      }),
    ).rejects.toMatchObject({ code: "SCOPE_MISMATCH" } satisfies Partial<AgentdBootstrapError>);
    expect(createRuntime).not.toHaveBeenCalled();
    expect(native.events).toEqual(["workspace.identity"]);
  });

  it("reports missing runtime composition only after validating launcher scope", async () => {
    const native = nativeHarness();
    const expectedScopeId = await expectedScope(native);

    await expect(
      startProductionAgentd({
        workspace: WORKSPACE,
        homeDirectory: HOME_DIRECTORY,
        effectiveConfigFingerprint: FINGERPRINT,
        expectedScopeId,
        nativeProvider: native.provider,
      }),
    ).rejects.toMatchObject({
      code: "RUNTIME_COMPOSITION_REQUIRED",
    } satisfies Partial<AgentdBootstrapError>);
    expect(native.events).toEqual(["workspace.identity"]);
  });
});
