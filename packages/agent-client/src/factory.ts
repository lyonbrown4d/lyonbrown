import { DefaultAgentClient } from "@lyonbrown/agent-client/internal/client";
import type { AgentClient, CreateAgentClientOptions } from "@lyonbrown/agent-client/internal/types";

export function createAgentClient(options: CreateAgentClientOptions): AgentClient {
  return new DefaultAgentClient(options);
}
