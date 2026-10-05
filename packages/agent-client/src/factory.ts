import { DefaultAgentClient } from "./client.ts";
import type { AgentClient, CreateAgentClientOptions } from "./types.ts";

export function createAgentClient(options: CreateAgentClientOptions): AgentClient {
  return new DefaultAgentClient(options);
}
