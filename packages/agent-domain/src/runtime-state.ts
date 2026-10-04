import { domainError } from "./errors.js";
import type { SessionId } from "./identifiers.js";
import { incrementRevision, type Revision } from "./revision.js";
import type { AgentRuntimeEvent, RuntimeUsage } from "./runtime-events.js";
import type { RuntimeJsonValue } from "./runtime-json.js";
import { checkedAssistantBytes, checkedUsageAdd } from "./runtime-state-budgets.js";

export type AgentRuntimePhase =
  | "uninitialized"
  | "ready"
  | "running"
  | "completed"
  | "cancelled"
  | "failed"
  | "failed_terminal";

export interface RuntimeFailure {
  readonly code: string;
  readonly message: string;
}

export interface RuntimeTurnState {
  readonly userText: string;
  readonly assistantText: string;
  readonly status: "submitted" | "running" | "completed" | "cancelled" | "failed";
  readonly turn?: number;
}

export interface RuntimeToolState {
  readonly callId: string;
  readonly toolName: string;
  readonly status: "running" | "succeeded" | "failed" | "outcome_unknown";
  readonly argumentsPreview?: RuntimeJsonValue;
  readonly content?: string;
  readonly details?: RuntimeJsonValue;
  readonly error?: RuntimeFailure;
}

export interface AgentRuntimeState {
  readonly sessionId: SessionId;
  readonly revision: Revision;
  readonly phase: AgentRuntimePhase;
  readonly turns: readonly RuntimeTurnState[];
  readonly tools: readonly RuntimeToolState[];
  readonly usage: RuntimeUsage;
  readonly assistantTextBytes: number;
  readonly lastError?: RuntimeFailure;
  readonly outcomeUnknown: boolean;
}

export interface VersionedAgentRuntimeEvent {
  readonly revision: Revision;
  readonly event: AgentRuntimeEvent;
}

const ZERO_USAGE: RuntimeUsage = Object.freeze({ inputTokens: 0, outputTokens: 0, totalTokens: 0 });

export function createInitialAgentRuntimeState(sessionId: SessionId): AgentRuntimeState {
  return freezeState({
    sessionId,
    revision: 0n as Revision,
    phase: "uninitialized",
    turns: [],
    tools: [],
    usage: ZERO_USAGE,
    assistantTextBytes: 0,
    outcomeUnknown: false,
  });
}

export function reduceAgentRuntimeEvent(
  state: AgentRuntimeState,
  input: VersionedAgentRuntimeEvent,
): AgentRuntimeState {
  if (input.event.sessionId !== state.sessionId) {
    throw domainError("failed_precondition", "Runtime event belongs to a different session", {
      reason: "session_mismatch",
      expected: state.sessionId,
      actual: input.event.sessionId,
    });
  }
  const expectedRevision = incrementRevision(state.revision);
  if (input.revision !== expectedRevision) {
    throw domainError("failed_precondition", "Runtime event revision is not contiguous", {
      reason: "revision_mismatch",
      expected: expectedRevision,
      actual: input.revision,
    });
  }
  if (state.phase === "uninitialized" && input.event.type !== "session_opened") {
    throw domainError("failed_precondition", "The first runtime event must be session_opened", {
      reason: "session_not_opened",
    });
  }
  if (state.phase !== "uninitialized" && input.event.type === "session_opened") {
    throw domainError("failed_precondition", "Runtime session is already opened", {
      reason: "session_already_opened",
    });
  }
  if (state.outcomeUnknown && input.event.type !== "run_failed") {
    throw domainError(
      "failed_precondition",
      "Runtime session is terminal after an uncertain outcome",
      {
        reason: "terminal_uncertain_outcome",
      },
    );
  }
  if (
    input.event.type === "user_message_submitted" &&
    (state.phase === "running" || state.tools.some((tool) => tool.status === "running"))
  ) {
    throw domainError("failed_precondition", "Runtime session has unresolved work", {
      reason: "unresolved_runtime_work",
    });
  }
  return applyEvent(state, input.revision, input.event);
}

function applyEvent(
  state: AgentRuntimeState,
  revision: Revision,
  event: AgentRuntimeEvent,
): AgentRuntimeState {
  switch (event.type) {
    case "session_opened":
      return freezeState({ ...state, revision, phase: "ready" });
    case "user_message_submitted":
      return freezeState({
        ...state,
        revision,
        phase: "ready",
        turns: [
          ...state.turns,
          freezeTurn({ userText: event.text, assistantText: "", status: "submitted" }),
        ],
        tools: [],
      });
    case "run_started":
      return updateCurrentTurn(state, revision, "running", { phase: "running" });
    case "turn_started":
      return updateCurrentTurn(state, revision, "running", { phase: "running", turn: event.turn });
    case "assistant_text_delta": {
      const assistantTextBytes = checkedAssistantBytes(state.assistantTextBytes, event.delta);
      return updateLastTurn(
        state,
        revision,
        (turn) => ({
          ...turn,
          status: "running",
          assistantText: `${turn.assistantText}${event.delta}`,
        }),
        { assistantTextBytes },
      );
    }
    case "usage_recorded":
      return freezeState({
        ...state,
        revision,
        usage: Object.freeze({
          inputTokens: checkedUsageAdd(state.usage.inputTokens, event.inputTokens),
          outputTokens: checkedUsageAdd(state.usage.outputTokens, event.outputTokens),
          totalTokens: checkedUsageAdd(state.usage.totalTokens, event.totalTokens),
        }),
      });
    case "turn_completed":
      return updateCurrentTurn(state, revision, "running", { turn: event.turn });
    case "run_completed":
      return updateCurrentTurn(state, revision, "completed", { phase: "completed" });
    case "run_cancelled":
      return updateCurrentTurn(state, revision, "cancelled", { phase: "cancelled" });
    case "tool_started":
      return freezeState({
        ...state,
        revision,
        phase: "running",
        tools: [
          ...state.tools.filter((tool) => tool.callId !== event.callId),
          freezeTool({
            callId: event.callId,
            toolName: event.toolName,
            status: "running",
            argumentsPreview: event.argumentsPreview,
          }),
        ],
      });
    case "tool_succeeded":
      return updateTool(state, revision, event.callId, {
        toolName: event.toolName,
        status: "succeeded",
        content: event.content,
        ...(event.details === undefined ? {} : { details: event.details }),
      });
    case "tool_failed":
      return updateTool(state, revision, event.callId, {
        toolName: event.toolName,
        status: "failed",
        error: freezeFailure(event),
      });
    case "tool_outcome_unknown":
      return freezeState({
        ...updateTool(state, revision, event.callId, {
          toolName: event.toolName,
          status: "outcome_unknown",
          error: freezeFailure(event),
        }),
        phase: "failed_terminal",
        lastError: freezeFailure(event),
        outcomeUnknown: true,
      });
    case "run_failed": {
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

function updateCurrentTurn(
  state: AgentRuntimeState,
  revision: Revision,
  status: RuntimeTurnState["status"],
  changes: Partial<Pick<AgentRuntimeState, "phase" | "lastError">> & { readonly turn?: number },
): AgentRuntimeState {
  const { turn, ...stateChanges } = changes;
  return updateLastTurn(
    state,
    revision,
    (current) => ({ ...current, status, ...(turn === undefined ? {} : { turn }) }),
    stateChanges,
  );
}

function updateLastTurn(
  state: AgentRuntimeState,
  revision: Revision,
  update: (turn: RuntimeTurnState) => RuntimeTurnState,
  changes: Partial<Pick<AgentRuntimeState, "assistantTextBytes" | "phase" | "lastError">> = {},
): AgentRuntimeState {
  const index = state.turns.length - 1;
  if (index < 0) return freezeState({ ...state, ...changes, revision });
  const turns = state.turns.map((turn, turnIndex) =>
    turnIndex === index ? freezeTurn(update(turn)) : turn,
  );
  return freezeState({ ...state, ...changes, revision, turns });
}

function updateTool(
  state: AgentRuntimeState,
  revision: Revision,
  callId: string,
  terminal: Omit<RuntimeToolState, "callId">,
): AgentRuntimeState {
  const existing = state.tools.find((tool) => tool.callId === callId);
  const replacement = freezeTool({
    ...(existing?.argumentsPreview === undefined
      ? {}
      : { argumentsPreview: existing.argumentsPreview }),
    callId,
    ...terminal,
  });
  const tools = existing
    ? state.tools.map((tool) => (tool.callId === callId ? replacement : tool))
    : [...state.tools, replacement];
  return freezeState({ ...state, revision, tools });
}

function freezeFailure(value: { readonly code: string; readonly message: string }): RuntimeFailure {
  return Object.freeze({ code: value.code, message: value.message });
}

function freezeTurn(turn: RuntimeTurnState): RuntimeTurnState {
  return Object.freeze(turn);
}

function freezeTool(tool: RuntimeToolState): RuntimeToolState {
  return Object.freeze(tool);
}

function freezeState(state: AgentRuntimeState): AgentRuntimeState {
  const result = {
    ...state,
    turns: Object.freeze([...state.turns]),
    tools: Object.freeze([...state.tools]),
    usage: Object.freeze({ ...state.usage }),
  };
  if (result.lastError === undefined) delete result.lastError;
  return Object.freeze(result);
}
