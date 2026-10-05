import { describe, expect, test } from "vitest";

import {
  asRevision,
  asSessionId,
  createInitialAgentRuntimeState,
  reduceAgentRuntimeEvent,
  type AgentRuntimeEvent,
  type AgentRuntimeState,
} from "../src/index.ts";

const sessionId = asSessionId("runtime-error-terminal");

function replay(events: readonly AgentRuntimeEvent[]): AgentRuntimeState {
  return events.reduce(
    (state, event, index) =>
      reduceAgentRuntimeEvent(state, { revision: asRevision(BigInt(index + 1)), event }),
    createInitialAgentRuntimeState(sessionId),
  );
}

function append(state: AgentRuntimeState, event: AgentRuntimeEvent): AgentRuntimeState {
  return reduceAgentRuntimeEvent(state, {
    revision: asRevision(state.revision + 1n),
    event,
  });
}

describe("runtime error terminal replay", () => {
  test("atomically fails the active turn and rejects every continuation of its tail", () => {
    const failed = replay([
      { type: "session_opened", sessionId },
      { type: "user_message_submitted", sessionId, text: "first" },
      { type: "run_started", sessionId },
      { type: "turn_started", sessionId, turn: 1 },
      { type: "follow_up_queued", sessionId, text: "second" },
      { type: "runtime_error", sessionId, code: "BROKEN", message: "runtime stopped" },
    ]);

    expect(failed).toMatchObject({
      phase: "failed_terminal",
      lastError: { code: "BROKEN", message: "runtime stopped" },
      turns: [{ status: "failed", activities: [{ status: "failed" }] }],
      queuedFollowUps: ["second"],
    });
    expect(() => append(failed, { type: "run_completed", sessionId })).toThrow(/terminal/u);
    expect(() =>
      append(failed, { type: "user_message_submitted", sessionId, text: "retry" }),
    ).toThrow(/terminal/u);
    expect(() => append(failed, { type: "follow_up_started", sessionId, text: "second" })).toThrow(
      /terminal/u,
    );
  });

  test("does not start another message or queued follow-up while a submitted turn is active", () => {
    const submitted = replay([
      { type: "session_opened", sessionId },
      { type: "user_message_submitted", sessionId, text: "first" },
      { type: "follow_up_queued", sessionId, text: "second" },
    ]);

    expect(() =>
      append(submitted, { type: "user_message_submitted", sessionId, text: "third" }),
    ).toThrow(/unresolved/u);
    expect(() =>
      append(submitted, { type: "follow_up_started", sessionId, text: "second" }),
    ).toThrow(/unresolved/u);
  });
});
