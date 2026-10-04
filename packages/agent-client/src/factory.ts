import { DefaultAgentClient } from "./client.js";
import type { AgentClient, CreateAgentClientOptions } from "./types.js";

export function createAgentClient(options: CreateAgentClientOptions): AgentClient {
  return new DefaultAgentClient(options);
}
