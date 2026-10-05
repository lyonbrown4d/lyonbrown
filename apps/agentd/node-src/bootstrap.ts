import { randomUUID } from "node:crypto";
import {
  ControlPlaneError,
  SecretToken,
  acquireLifecycleLock,
  createDiscoveryDescriptor,
  deriveScopedDaemonPaths,
  openSecureRunDirectory,
  type DaemonLease,
  type NativeControlPlaneProvider,
  type SecureRunDirectory,
} from "@lyonbrown/local-control-plane";
import type { RuntimeHost } from "./runtime-host.ts";
import {
  startAgentdServer,
  type RunningAgentdServer,
  type StartAgentdServerOptions,
} from "./server.ts";

export type AgentdBootstrapErrorCode =
  | "DAEMON_ALREADY_RUNNING"
  | "RUNTIME_COMPOSITION_REQUIRED"
  | "SCOPE_MISMATCH";

export class AgentdBootstrapError extends Error {
  public override readonly name = "AgentdBootstrapError";

  public constructor(public readonly code: AgentdBootstrapErrorCode) {
    super(
      code === "DAEMON_ALREADY_RUNNING"
        ? "another daemon owns the workspace lifecycle lease"
        : code === "SCOPE_MISMATCH"
          ? "launcher scope does not match the derived daemon scope"
          : "runtime composition is required",
    );
  }
}

export interface ProductionRuntimeContext {
  readonly canonicalWorkspace: string;
  readonly dataDirectory: string;
  readonly scopeId: string;
}

export interface ProductionAgentdRuntime {
  readonly host: RuntimeHost;
  close(): Promise<void>;
}

export interface ProductionAgentdOptions {
  readonly workspace: string;
  readonly homeDirectory: string;
  readonly effectiveConfigFingerprint: string;
  readonly expectedScopeId: string;
  readonly nativeProvider?: NativeControlPlaneProvider;
  readonly createRuntime?: (context: ProductionRuntimeContext) => Promise<ProductionAgentdRuntime>;
}

export interface RunningProductionAgentd extends RunningAgentdServer {
  readonly scopeId: string;
}

export interface ProductionAgentdDependencies {
  readonly startServer: (options: StartAgentdServerOptions) => Promise<RunningAgentdServer>;
}

const DEFAULT_DEPENDENCIES: ProductionAgentdDependencies = Object.freeze({
  startServer: startAgentdServer,
});

interface OwnedResources {
  readonly directory: SecureRunDirectory | undefined;
  readonly lease: DaemonLease | undefined;
  readonly runtime: ProductionAgentdRuntime | undefined;
  readonly server: RunningAgentdServer | undefined;
}

export async function startProductionAgentd(
  options: ProductionAgentdOptions,
  dependencies: ProductionAgentdDependencies = DEFAULT_DEPENDENCIES,
): Promise<RunningProductionAgentd> {
  if (options.nativeProvider === undefined) {
    throw new ControlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
  }
  const paths = await deriveScopedDaemonPaths(
    options.homeDirectory,
    options.workspace,
    options.effectiveConfigFingerprint,
    options.nativeProvider,
  );
  if (options.expectedScopeId !== paths.scopeId) {
    throw new AgentdBootstrapError("SCOPE_MISMATCH");
  }
  if (options.createRuntime === undefined) {
    throw new AgentdBootstrapError("RUNTIME_COMPOSITION_REQUIRED");
  }
  let directory: SecureRunDirectory | undefined;
  let lease: DaemonLease | undefined;
  let runtime: ProductionAgentdRuntime | undefined;
  let server: RunningAgentdServer | undefined;
  try {
    directory = await openSecureRunDirectory(paths.runDirectory, options.nativeProvider);
    const acquiredLease = await acquireLifecycleLock(directory);
    if (acquiredLease === null) throw new AgentdBootstrapError("DAEMON_ALREADY_RUNNING");
    lease = acquiredLease;
    runtime = await options.createRuntime({
      canonicalWorkspace: paths.canonicalWorkspace,
      dataDirectory: paths.dataDirectory,
      scopeId: paths.scopeId,
    });
    const token = SecretToken.generate();
    const serverInstanceId = randomUUID();
    server = await dependencies.startServer({
      host: runtime.host,
      token,
      serverInstanceId,
      scopeFingerprint: paths.scopeId,
    });
    await directory.discovery.publish(
      createDiscoveryDescriptor({
        endpoint: server.address,
        pid: process.pid,
        scopeId: paths.scopeId,
        serverInstanceId,
        token,
      }),
    );
  } catch (error: unknown) {
    const cleanupFailures = await closeOwned({ directory, lease, runtime, server });
    if (cleanupFailures.length > 0) {
      throw new AggregateError(
        [error, ...cleanupFailures],
        "agentd startup failed and resource cleanup was incomplete",
        { cause: error },
      );
    }
    throw error;
  }

  const owned = { directory, lease, runtime, server };
  let closePromise: Promise<void> | undefined;
  return Object.freeze({
    address: server.address,
    port: server.port,
    scopeId: paths.scopeId,
    close(): Promise<void> {
      closePromise ??= closeProductionResources(owned);
      return closePromise;
    },
  });
}

async function closeProductionResources(resources: OwnedResources): Promise<void> {
  const failures = await closeOwned(resources);
  if (failures.length > 0) {
    throw new AggregateError(failures, "agentd resource cleanup failed");
  }
}

async function closeOwned(resources: OwnedResources): Promise<unknown[]> {
  const failures: unknown[] = [];
  for (const close of [
    resources.server?.close.bind(resources.server),
    resources.runtime?.close.bind(resources.runtime),
    resources.lease?.release.bind(resources.lease),
    resources.directory?.close.bind(resources.directory),
  ]) {
    if (close === undefined) continue;
    try {
      await close();
    } catch (error: unknown) {
      failures.push(error);
    }
  }
  return failures;
}
