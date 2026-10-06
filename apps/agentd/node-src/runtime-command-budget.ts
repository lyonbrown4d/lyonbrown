import type { AgentRuntimeState } from "@lyonbrown/agent-domain";

import type { RuntimeCommand } from "./runtime-host.ts";
import { followUpWithinBudget, steeringWithinBudget } from "./runtime-session-policy.ts";

export function runtimeCommandWithinBudget(
  state: AgentRuntimeState,
  command: RuntimeCommand,
): boolean {
  if (command.kind === "queue-follow-up") return followUpWithinBudget(state, command.text);
  if (command.kind === "steer-current-turn") return steeringWithinBudget(state, command.text);
  return true;
}
