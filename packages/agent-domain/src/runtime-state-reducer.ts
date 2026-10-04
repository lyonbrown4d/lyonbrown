import { domainError } from "./errors.js";
import type { Revision } from "./revision.js";
import type { AgentRuntimeEvent, RuntimeUsage } from "./runtime-events.js";
import { checkedAssistantBytes, checkedUsageAdd } from "./runtime-state-budgets.js";
import {
  addTool,
  appendActivity,
  appendAssistantDelta,
  appendTurn,
  hasActiveTurn,
  updateCurrentTurn,
  updateLastActivity,
  updateLastTurn,
  updateTool,
} from "./runtime-state-mutations.js";
import type { AgentRuntimeState } from "./runtime-state-types.js";
import { freezeFailure, freezeState } from "./runtime-state-types.js";

export function applyRuntimeEvent(
  state: AgentRuntimeState,
  revision: Revision,
  event: AgentRuntimeEvent,
): AgentRuntimeState {
  switch (event.type) {
    case "session_opened":
      return freezeState({ ...state, revision, phase: "ready" });
    case "user_message_submitted":
      return appendTurn(state, revision, event.text);
    case "steering_submitted":
      if (!hasActiveTurn(state)) throw noActiveTurn();
      return updateLastTurn(state, revision, (turn) => ({
        ...turn,
        steering: [...turn.steering, event.text],
      }));
    case "follow_up_queued":
      return freezeState({
        ...state,
        revision,
        queuedFollowUps: [...state.queuedFollowUps, event.text],
      });
    case "follow_up_started":
      assertFollowUpCanStart(state, event.text);
      return appendTurn(
        { ...state, queuedFollowUps: state.queuedFollowUps.slice(1) },
        revision,
        event.text,
      );
    case "run_started":
      assertActiveTurn(state);
      return updateCurrentTurn(state, revision, "running", { phase: "running" });
    case "turn_started":
      assertActiveTurn(state);
      return updateLastTurn(state, revision, (turn) => appendActivity(turn, event.turn), {
        phase: "running",
      });
    case "assistant_text_delta":
      assertActiveTurn(state);
      return updateLastTurn(state, revision, (turn) => appendAssistantDelta(turn, event.delta), {
        assistantTextBytes: checkedAssistantBytes(state.assistantTextBytes, event.delta),
      });
    case "usage_recorded":
      assertActiveTurn(state);
      return recordUsage(state, revision, event);
    case "turn_completed":
      assertActiveTurn(state);
      return updateLastTurn(state, revision, (turn) => ({
        ...updateLastActivity(turn, (activity) => ({ ...activity, status: "completed" })),
        status: "running",
        turn: event.turn,
      }));
    case "run_completed":
      assertActiveTurn(state);
      return updateCurrentTurn(state, revision, "completed", { phase: "completed" });
    case "run_cancelled":
      assertActiveTurn(state);
      return updateCurrentTurn(state, revision, "cancelled", { phase: "cancelled" });
    case "tool_started":
      assertActiveTurn(state);
      return addTool(state, revision, {
        callId: event.callId,
        toolName: event.toolName,
        status: "running",
        argumentsPreview: event.argumentsPreview,
      });
    case "tool_succeeded":
      assertActiveTurn(state);
      return updateTool(state, revision, event.callId, {
        toolName: event.toolName,
        status: "succeeded",
        content: event.content,
        ...(event.details === undefined ? {} : { details: event.details }),
      });
    case "tool_failed":
      assertActiveTurn(state);
      return updateTool(state, revision, event.callId, {
        toolName: event.toolName,
        status: "failed",
        error: freezeFailure(event),
      });
    case "tool_outcome_unknown": {
      assertActiveTurn(state);
      const withTool = updateTool(state, revision, event.callId, {
        toolName: event.toolName,
        status: "outcome_unknown",
        error: freezeFailure(event),
      });
      return updateLastTurn(
        withTool,
        revision,
        (turn) => ({
          ...updateLastActivity(turn, (activity) => ({ ...activity, status: "failed" })),
          status: "failed",
        }),
        {
          phase: "failed_terminal",
          lastError: freezeFailure(event),
          outcomeUnknown: true,
        },
      );
    }
    case "run_failed": {
      if (!state.outcomeUnknown) assertActiveTurn(state);
      const failure = freezeFailure(event);
      return updateCurrentTurn(state, revision, "failed", {
        phase: state.outcomeUnknown ? "failed_terminal" : "failed",
        lastError: state.lastError ?? failure,
      });
    }
    case "runtime_error": {
      const failure = freezeFailure(event);
      return freezeState({ ...state, revision, phase: "failed", lastError: failure });
    }
  }
}

function recordUsage(
  state: AgentRuntimeState,
  revision: Revision,
  event: Extract<AgentRuntimeEvent, { readonly type: "usage_recorded" }>,
): AgentRuntimeState {
  const usage = addUsage(state.usage, event);
  return updateLastTurn(
    state,
    revision,
    (turn) =>
      updateLastActivity(turn, (activity) => ({
        ...activity,
        usage: addUsage(activity.usage, event),
      })),
    { usage },
  );
}

function addUsage(current: RuntimeUsage, delta: RuntimeUsage): RuntimeUsage {
  return Object.freeze({
    inputTokens: checkedUsageAdd(current.inputTokens, delta.inputTokens),
    outputTokens: checkedUsageAdd(current.outputTokens, delta.outputTokens),
    totalTokens: checkedUsageAdd(current.totalTokens, delta.totalTokens),
  });
}

function assertFollowUpCanStart(state: AgentRuntimeState, text: string): void {
  if (state.phase === "running" || state.tools.some((tool) => tool.status === "running")) {
    throw domainError("failed_precondition", "Runtime session has unresolved work", {
      reason: "unresolved_runtime_work",
    });
  }
  if (state.queuedFollowUps[0] !== text) {
    throw domainError("failed_precondition", "Follow-up does not match the durable queue", {
      reason: "follow_up_queue_mismatch",
    });
  }
}

function assertActiveTurn(state: AgentRuntimeState): void {
  if (!hasActiveTurn(state)) throw noActiveTurn();
}

function noActiveTurn() {
  return domainError("failed_precondition", "Runtime session has no active turn", {
    reason: "no_active_turn",
  });
}
