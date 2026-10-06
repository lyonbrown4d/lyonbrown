import type { AgentRuntimeState } from "@lyonbrown/agent-domain";

import {
  MAX_QUEUED_FOLLOW_UP_BYTES,
  MAX_QUEUED_FOLLOW_UPS,
  MAX_SESSION_STEERING_BYTES,
  MAX_SESSION_STEERING_PER_TURN,
  MAX_SESSION_TURNS,
  MAX_SESSION_USER_BYTES,
} from "./runtime-limits.ts";

function textBytes(text: string): number {
  return Buffer.byteLength(text, "utf8");
}

function sessionUserBytes(state: AgentRuntimeState): number {
  let bytes = 0;
  for (const turn of state.turns) {
    bytes += textBytes(turn.userText);
    for (const steering of turn.steering) bytes += textBytes(steering);
  }
  for (const followUp of state.queuedFollowUps) bytes += textBytes(followUp);
  return bytes;
}

function exceedsSessionBytes(state: AgentRuntimeState, nextInput: string): boolean {
  return sessionUserBytes(state) + textBytes(nextInput) > MAX_SESSION_USER_BYTES;
}

function exceedsSessionTurns(state: AgentRuntimeState): boolean {
  return state.turns.length + state.queuedFollowUps.length + 1 > MAX_SESSION_TURNS;
}

export function runtimeHasActiveTurn(state: AgentRuntimeState): boolean {
  return (
    state.phase === "running" ||
    state.turns.some((turn) => turn.status === "submitted" || turn.status === "running")
  );
}

export function shouldRotateSession(state: AgentRuntimeState, nextInput: string): boolean {
  if (runtimeHasActiveTurn(state)) return false;
  return exceedsSessionTurns(state) || exceedsSessionBytes(state, nextInput);
}

export function followUpWithinBudget(state: AgentRuntimeState, nextInput: string): boolean {
  if (exceedsSessionTurns(state) || exceedsSessionBytes(state, nextInput)) return false;
  if (state.queuedFollowUps.length >= MAX_QUEUED_FOLLOW_UPS) return false;
  let bytes = textBytes(nextInput);
  for (const followUp of state.queuedFollowUps) {
    bytes += textBytes(followUp);
    if (bytes > MAX_QUEUED_FOLLOW_UP_BYTES) return false;
  }
  return true;
}

export function steeringWithinBudget(state: AgentRuntimeState, nextInput: string): boolean {
  if (!runtimeHasActiveTurn(state)) return true;
  const turn = state.turns.at(-1);
  if (turn === undefined || turn.steering.length >= MAX_SESSION_STEERING_PER_TURN) return false;
  let steeringBytes = textBytes(nextInput);
  for (const steering of turn.steering) steeringBytes += textBytes(steering);
  return steeringBytes <= MAX_SESSION_STEERING_BYTES && !exceedsSessionBytes(state, nextInput);
}
