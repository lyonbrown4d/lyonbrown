import type { AgentClient, ClientDeadlines, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { NativeControlPlaneProvider, ScopeIdentity } from "@lyonbrown/local-control-plane";

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
  release(): Promise<void>;
}

export interface DiscoveryClient {
  read(context: DiscoveryContext): Promise<DiscoveredDaemonCapability | null>;
  tryAcquireStartupLease(context: DiscoveryContext): Promise<StartupLease | null>;
}

export interface SpawnAgentdRequest {
  readonly canonicalWorkspace: string;
  readonly effectiveConfigFingerprint: string;
  readonly scopeId: string;
}

export interface DaemonExit {
  readonly exitCode: number | null;
}

export interface SpawnedDaemon {
  readonly exited: Promise<DaemonExit>;
}

export interface ProcessSpawner {
  spawn(request: SpawnAgentdRequest): Promise<SpawnedDaemon>;
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

export interface ConnectAgentInput {
  readonly autoStart?: boolean;
  readonly signal?: AbortSignal;
}

export interface AgentLaunchConnection {
  readonly client: AgentClient;
  readonly serverInstanceId: string;
  readonly scopeId: string;
  readonly snapshot: RuntimeSnapshotDto;
  readonly source: "reused" | "spawned";
}

export interface AgentLauncher {
  connect(input?: ConnectAgentInput): Promise<AgentLaunchConnection>;
}
