import type { AgentClient, RuntimeSnapshotDto } from "@lyonbrown/agent-client";

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
