import type { SessionId } from "./identifiers.js";
import type { Revision } from "./revision.js";
import type { RuntimeUsage } from "./runtime-events.js";
import type { RuntimeJsonValue } from "./runtime-json.js";

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

export interface RuntimeToolState {
  readonly callId: string;
  readonly toolName: string;
  readonly status: "running" | "succeeded" | "failed" | "outcome_unknown";
  readonly argumentsPreview?: RuntimeJsonValue;
  readonly content?: string;
  readonly details?: RuntimeJsonValue;
  readonly error?: RuntimeFailure;
}

export interface RuntimeActivityState {
  readonly turn: number;
  readonly assistantText: string;
  readonly status: "running" | "completed";
  readonly usage: RuntimeUsage;
  readonly tools: readonly RuntimeToolState[];
}

export interface RuntimeTurnState {
  readonly userText: string;
  readonly assistantText: string;
  readonly status: "submitted" | "running" | "completed" | "cancelled" | "failed";
  readonly turn?: number;
  readonly steering: readonly string[];
  readonly activities: readonly RuntimeActivityState[];
}

export interface AgentRuntimeState {
  readonly sessionId: SessionId;
  readonly revision: Revision;
  readonly phase: AgentRuntimePhase;
  readonly turns: readonly RuntimeTurnState[];
  readonly tools: readonly RuntimeToolState[];
  readonly queuedFollowUps: readonly string[];
  readonly usage: RuntimeUsage;
  readonly assistantTextBytes: number;
  readonly lastError?: RuntimeFailure;
  readonly outcomeUnknown: boolean;
}

export const ZERO_RUNTIME_USAGE: RuntimeUsage = Object.freeze({
  inputTokens: 0,
  outputTokens: 0,
  totalTokens: 0,
});

export function freezeFailure(value: {
  readonly code: string;
  readonly message: string;
}): RuntimeFailure {
  return Object.freeze({ code: value.code, message: value.message });
}

export function freezeTool(tool: RuntimeToolState): RuntimeToolState {
  return Object.freeze(tool);
}

export function freezeActivity(activity: RuntimeActivityState): RuntimeActivityState {
  return Object.freeze({
    ...activity,
    usage: Object.freeze({ ...activity.usage }),
    tools: Object.freeze(activity.tools.map(freezeTool)),
  });
}

export function freezeTurn(turn: RuntimeTurnState): RuntimeTurnState {
  return Object.freeze({
    ...turn,
    steering: Object.freeze([...turn.steering]),
    activities: Object.freeze(turn.activities.map(freezeActivity)),
  });
}

export function freezeState(state: AgentRuntimeState): AgentRuntimeState {
  const result = {
    ...state,
    turns: Object.freeze([...state.turns]),
    tools: Object.freeze([...state.tools]),
    queuedFollowUps: Object.freeze([...state.queuedFollowUps]),
    usage: Object.freeze({ ...state.usage }),
  };
  if (result.lastError === undefined) delete result.lastError;
  return Object.freeze(result);
}
