import type { AgentClient, ClientDeadlines } from "@lyonbrown/agent-client";
import type { NativeControlPlaneProvider, ScopeIdentity } from "@lyonbrown/local-control-plane";
export type {
  AgentLaunchConnection,
  AgentLauncher,
  ConnectAgentInput,
} from "@lyonbrown/agent-launch/internal/public-types";

/** Capability whose implementation privately retains authenticated connection material. */
export interface DiscoveredDaemonCapability {
  readonly serverInstanceId: string;
  readonly scopeId: string;
}

export interface DiscoveryContext {
  readonly nativeProvider: NativeControlPlaneProvider;
  readonly scope: ScopeIdentity;
}

export interface StartupLease {
  /** Release must observe cancellation and must remain safe if completion is reported late. */
  release(signal: AbortSignal): Promise<void>;
}

export interface DiscoveryClient {
  /** A cancelled read must not publish a new descriptor after it settles. */
  read(context: DiscoveryContext, signal: AbortSignal): Promise<DiscoveredDaemonCapability | null>;
  /** A lease returned after cancellation is fenced by the caller and released exactly once. */
  tryAcquireStartupLease(
    context: DiscoveryContext,
    signal: AbortSignal,
  ): Promise<StartupLease | null>;
}

export interface SpawnAgentdRequest {
  readonly canonicalWorkspace: string;
  readonly effectiveConfigFingerprint: string;
  readonly scopeId: string;
}

export interface DaemonExit {
  readonly exitCode: number | null;
}

export interface SpawnAttempt {
  readonly exited: Promise<DaemonExit>;
  /** Fulfills only after the attempt can no longer commit or own a daemon process. */
  readonly retired: Promise<void>;
  /** Retire a startup attempt without acting on unrelated stale descriptors or processes. */
  abandon(signal: AbortSignal): Promise<void>;
}

export interface ProcessSpawner {
  /** Return the attempt handle synchronously before any background process commit is possible. */
  begin(request: SpawnAgentdRequest): SpawnAttempt;
}

export interface AgentClientFactory {
  create(candidate: DiscoveredDaemonCapability, deadlines?: ClientDeadlines): AgentClient;
  replaceBinding(client: AgentClient, candidate: DiscoveredDaemonCapability): void;
}

export interface Clock {
  /** Monotonic milliseconds. */
  now(): number;
}

export interface Sleeper {
  /** Must reject promptly when `signal` aborts, including for a full-deadline wait. */
  sleep(durationMs: number, signal?: AbortSignal): Promise<void>;
}

export interface AgentLaunchPorts {
  readonly discovery: DiscoveryClient;
  readonly spawner: ProcessSpawner;
  readonly clients: AgentClientFactory;
  readonly clock: Clock;
  readonly sleeper: Sleeper;
}

export interface AgentLaunchDeadlines {
  readonly startupMs?: number;
  readonly pollIntervalMs?: number;
  readonly cleanupMs?: number;
  readonly client?: ClientDeadlines;
}

export interface CreateAgentLauncherOptions {
  readonly nativeProvider: NativeControlPlaneProvider;
  readonly workspace: string;
  readonly effectiveConfigFingerprint: string;
  readonly ports: AgentLaunchPorts;
  readonly deadlines?: AgentLaunchDeadlines;
  readonly client: {
    readonly name: string;
    readonly version: string;
  };
}
