import type { AgentRuntimeState, RuntimeActivityState } from "@lyonbrown/agent-domain";
import {
  isValidKernelConversationHistory,
  type KernelConversationMessage,
} from "@lyonbrown/kernel-pi";

import { PRODUCTION_KERNEL_BUDGET } from "@lyonbrown/agentd/internal/runtime-limits";

export function runtimeHistory(
  state: AgentRuntimeState,
): readonly KernelConversationMessage[] | undefined {
  if (!isResumable(state)) return undefined;
  const messages: KernelConversationMessage[] = [];
  for (const turn of state.turns) {
    messages.push({ role: "user", content: turn.userText });
    if (turn.activities.length === 0) {
      messages.push({ role: "assistant", content: turn.assistantText, toolCalls: [] });
      continue;
    }
    for (const activity of turn.activities) {
      const mapped = activityMessages(activity);
      if (mapped === undefined) return undefined;
      messages.push(...mapped);
    }
  }
  if (!isValidKernelConversationHistory(messages, PRODUCTION_KERNEL_BUDGET)) return undefined;
  return Object.freeze(messages);
}

function isResumable(state: AgentRuntimeState): boolean {
  if (
    (state.phase !== "ready" && state.phase !== "completed") ||
    state.pendingApproval !== undefined ||
    state.outcomeUnknown ||
    state.tools.some((tool) => tool.status === "running" || tool.status === "outcome_unknown")
  ) {
    return false;
  }
  return state.turns.every(
    (turn) =>
      turn.status === "completed" &&
      turn.steering.length === 0 &&
      turn.activities.every(
        (activity) =>
          activity.status === "completed" &&
          activity.tools.every((tool) => tool.status === "succeeded" || tool.status === "failed"),
      ),
  );
}

function activityMessages(
  activity: RuntimeActivityState,
): readonly KernelConversationMessage[] | undefined {
  const tools = [];
  for (const tool of activity.tools) {
    const arguments_ = asArguments(tool.argumentsPreview);
    if (arguments_ === undefined) return undefined;
    tools.push({ callId: tool.callId, name: tool.toolName, arguments: arguments_ });
  }
  return [
    { role: "assistant", content: activity.assistantText, toolCalls: tools },
    ...activity.tools.map(
      (tool): KernelConversationMessage => ({
        role: "tool_result",
        callId: tool.callId,
        name: tool.toolName,
        content:
          tool.status === "succeeded" ? (tool.content ?? "") : (tool.error?.message ?? "failed"),
        isError: tool.status === "failed",
      }),
    ),
  ];
}

function asArguments(
  value: RuntimeActivityState["tools"][number]["argumentsPreview"],
): Readonly<Record<string, unknown>> | undefined {
  if (value === undefined || value === null || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  return value as Readonly<Record<string, unknown>>;
}
