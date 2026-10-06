import type { Revision } from "@lyonbrown/agent-domain/internal/revision";
import type {
  AgentRuntimeState,
  RuntimeActivityState,
  RuntimeToolState,
  RuntimeTurnState,
} from "@lyonbrown/agent-domain/internal/runtime-state-types";
import {
  ZERO_RUNTIME_USAGE,
  freezeActivity,
  freezeState,
  freezeTool,
  freezeTurn,
} from "@lyonbrown/agent-domain/internal/runtime-state-types";

type StateChanges = Partial<
  Pick<AgentRuntimeState, "assistantTextBytes" | "outcomeUnknown" | "phase" | "lastError" | "usage">
>;

export function appendTurn(
  state: AgentRuntimeState,
  revision: Revision,
  userText: string,
): AgentRuntimeState {
  return freezeState({
    ...state,
    revision,
    phase: "ready",
    turns: [
      ...state.turns,
      freezeTurn({
        userText,
        assistantText: "",
        status: "submitted",
        steering: [],
        activities: [],
      }),
    ],
    tools: [],
  });
}

export function updateCurrentTurn(
  state: AgentRuntimeState,
  revision: Revision,
  status: RuntimeTurnState["status"],
  changes: Partial<Pick<AgentRuntimeState, "phase" | "lastError">> & {
    readonly turn?: number;
  },
): AgentRuntimeState {
  const { turn, ...stateChanges } = changes;
  return updateLastTurn(
    state,
    revision,
    (current) => ({ ...current, status, ...(turn === undefined ? {} : { turn }) }),
    stateChanges,
  );
}

export function updateLastTurn(
  state: AgentRuntimeState,
  revision: Revision,
  update: (turn: RuntimeTurnState) => RuntimeTurnState,
  changes: StateChanges = {},
): AgentRuntimeState {
  const index = state.turns.length - 1;
  if (index < 0) return freezeState({ ...state, ...changes, revision });
  const turns = state.turns.map((turn, turnIndex) =>
    turnIndex === index ? freezeTurn(update(turn)) : turn,
  );
  return freezeState({ ...state, ...changes, revision, turns });
}

export function appendActivity(turn: RuntimeTurnState, activityTurn: number): RuntimeTurnState {
  return {
    ...turn,
    status: "running",
    turn: activityTurn,
    activities: [
      ...turn.activities,
      freezeActivity({
        turn: activityTurn,
        assistantText: "",
        status: "running",
        usage: ZERO_RUNTIME_USAGE,
        tools: [],
      }),
    ],
  };
}

export function appendAssistantDelta(turn: RuntimeTurnState, delta: string): RuntimeTurnState {
  return updateLastActivity(
    { ...turn, status: "running", assistantText: `${turn.assistantText}${delta}` },
    (activity) => ({ ...activity, assistantText: `${activity.assistantText}${delta}` }),
  );
}

export function updateLastActivity(
  turn: RuntimeTurnState,
  update: (activity: RuntimeActivityState) => RuntimeActivityState,
): RuntimeTurnState {
  const index = turn.activities.length - 1;
  if (index < 0) return turn;
  return {
    ...turn,
    activities: turn.activities.map((activity, activityIndex) =>
      activityIndex === index ? freezeActivity(update(activity)) : activity,
    ),
  };
}

export function addTool(
  state: AgentRuntimeState,
  revision: Revision,
  tool: RuntimeToolState,
): AgentRuntimeState {
  const replacement = freezeTool(tool);
  const tools = replaceTool(state.tools, replacement);
  return updateLastTurn({ ...state, phase: "running", tools }, revision, (turn) =>
    updateLastActivity(turn, (activity) => ({
      ...activity,
      tools: replaceTool(activity.tools, replacement),
    })),
  );
}

export function updateTool(
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
  const tools = replaceTool(state.tools, replacement);
  return updateLastTurn({ ...state, tools }, revision, (turn) =>
    updateLastActivity(turn, (activity) => ({
      ...activity,
      tools: replaceTool(activity.tools, replacement),
    })),
  );
}

export function hasActiveTurn(state: AgentRuntimeState): boolean {
  const status = state.turns.at(-1)?.status;
  return status === "submitted" || status === "running";
}

function replaceTool(
  tools: readonly RuntimeToolState[],
  replacement: RuntimeToolState,
): readonly RuntimeToolState[] {
  const existing = tools.some((tool) => tool.callId === replacement.callId);
  return existing
    ? tools.map((tool) => (tool.callId === replacement.callId ? replacement : tool))
    : [...tools, replacement];
}
