import { domainError } from "@lyonbrown/agent-domain/internal/errors";
import type { Revision } from "@lyonbrown/agent-domain/internal/revision";
import type {
  AgentRuntimeEvent,
  RuntimeUsage,
} from "@lyonbrown/agent-domain/internal/runtime-events";
import {
  checkedAssistantBytes,
  checkedUsageAdd,
} from "@lyonbrown/agent-domain/internal/runtime-state-budgets";
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
} from "@lyonbrown/agent-domain/internal/runtime-state-mutations";
import type { AgentRuntimeState } from "@lyonbrown/agent-domain/internal/runtime-state-types";
import { freezeFailure, freezeState } from "@lyonbrown/agent-domain/internal/runtime-state-types";

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
    case "decision_recorded":
    case "decision_fallback_recorded":
      assertActiveTurn(state);
      return freezeState({ ...state, revision });
    case "model_selection_changed":
      return freezeState({
        ...state,
        revision,
        modelSelection:
          event.mode === "auto"
            ? { kind: "auto" }
            : { kind: "pinned", provider: event.provider, model: event.model },
      });
    case "approval_requested":
      assertActiveTurn(state);
      if (state.pendingApproval !== undefined) {
        throw domainError("failed_precondition", "Runtime session already has a pending approval", {
          reason: "approval_already_pending",
        });
      }
      return freezeState({
        ...state,
        revision,
        pendingApproval: {
          requestId: event.requestId,
          toolName: event.toolName,
          risk: event.risk,
          permissions: event.permissions,
          argumentsPreview: event.argumentsPreview,
        },
      });
    case "approval_resolved":
      assertActiveTurn(state);
      if (state.pendingApproval?.requestId !== event.requestId) {
        throw domainError(
          "failed_precondition",
          "Approval resolution does not match pending approval",
          {
            reason: "approval_request_mismatch",
          },
        );
      }
      return resolveApproval(state, revision, event.decision);
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
      if (!hasActiveTurn(state)) {
        return freezeState({
          ...state,
          revision,
          phase: "failed_terminal",
          lastError: failure,
        });
      }
      return updateLastTurn(
        state,
        revision,
        (turn) => ({
          ...updateLastActivity(turn, (activity) => ({ ...activity, status: "failed" })),
          status: "failed",
        }),
        { phase: "failed_terminal", lastError: failure },
      );
    }
  }
}

function resolveApproval(
  state: AgentRuntimeState,
  revision: Revision,
  decision: Extract<AgentRuntimeEvent, { readonly type: "approval_resolved" }>["decision"],
): AgentRuntimeState {
  const { pendingApproval, ...remaining } = state;
  if (pendingApproval === undefined) throw noActiveTurn();
  const allowedTools =
    decision === "allow-for-session" &&
    !state.approvalPolicy.allowedTools.includes(pendingApproval.toolName)
      ? [...state.approvalPolicy.allowedTools, pendingApproval.toolName]
      : state.approvalPolicy.allowedTools;
  return freezeState({
    ...remaining,
    revision,
    approvalPolicy: {
      allowedTools,
      denyRemaining: state.approvalPolicy.denyRemaining || decision === "deny-remaining",
    },
  });
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
  if (
    hasActiveTurn(state) ||
    state.phase === "running" ||
    state.tools.some((tool) => tool.status === "running")
  ) {
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
