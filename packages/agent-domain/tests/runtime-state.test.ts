import { describe, expect, test } from "vitest";

import {
  MAX_AGENT_RUNTIME_ASSISTANT_BYTES,
  asRevision,
  asSessionId,
  createInitialAgentRuntimeState,
  reduceAgentRuntimeEvent,
  type AgentRuntimeEvent,
} from "../src/index.js";

const sessionId = asSessionId("session-runtime-state");

function apply(
  state: ReturnType<typeof createInitialAgentRuntimeState>,
  revision: bigint,
  event: AgentRuntimeEvent,
) {
  return reduceAgentRuntimeEvent(state, { revision: asRevision(revision), event });
}

describe("agent runtime reducer", () => {
  test("projects a complete text turn without mutating prior snapshots", () => {
    const initial = createInitialAgentRuntimeState(sessionId);
    const opened = apply(initial, 1n, { type: "session_opened", sessionId });
    const submitted = apply(opened, 2n, {
      type: "user_message_submitted",
      sessionId,
      text: "question",
    });
    const started = apply(submitted, 3n, { type: "run_started", sessionId });
    const turnStarted = apply(started, 4n, {
      type: "turn_started",
      sessionId,
      turn: 1,
    });
    const delta = apply(turnStarted, 5n, {
      type: "assistant_text_delta",
      sessionId,
      delta: "answer",
    });
    const completed = apply(delta, 6n, { type: "run_completed", sessionId });

    expect(completed).toMatchObject({
      phase: "completed",
      revision: 6n,
      outcomeUnknown: false,
      turns: [
        {
          userText: "question",
          assistantText: "answer",
          status: "completed",
          turn: 1,
        },
      ],
    });
    expect(turnStarted.turns[0]?.assistantText).toBe("");
    expect(initial.phase).toBe("uninitialized");
    expect(Object.isFrozen(completed)).toBe(true);
    expect(Object.isFrozen(completed.turns)).toBe(true);
  });

  test("makes an uncertain tool outcome terminal", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, {
      type: "user_message_submitted",
      sessionId,
      text: "change it",
    });
    state = apply(state, 3n, { type: "run_started", sessionId });
    state = apply(state, 4n, { type: "turn_started", sessionId, turn: 1 });
    state = apply(state, 5n, {
      type: "tool_started",
      sessionId,
      callId: "call-1",
      toolName: "write_file",
      argumentsPreview: { path: "README.md" },
    });
    state = apply(state, 6n, {
      type: "tool_outcome_unknown",
      sessionId,
      callId: "call-1",
      toolName: "write_file",
      code: "TOOL_TIMEOUT",
      message: "Tool outcome is unknown",
    });

    expect(state.phase).toBe("failed_terminal");
    expect(state.outcomeUnknown).toBe(true);
    expect(state.turns[0]).toMatchObject({
      status: "failed",
      activities: [{ status: "failed" }],
    });
    expect(state.tools).toEqual([
      expect.objectContaining({ callId: "call-1", status: "outcome_unknown" }),
    ]);
    expect(state.lastError).toEqual({
      code: "TOOL_TIMEOUT",
      message: "Tool outcome is unknown",
    });
  });

  test("rejects gaps, session changes, and events before SessionOpened", () => {
    const initial = createInitialAgentRuntimeState(sessionId);

    expect(() => apply(initial, 1n, { type: "run_started", sessionId })).toThrow(/session_opened/u);
    expect(() => apply(initial, 2n, { type: "session_opened", sessionId })).toThrow(/revision/u);
    expect(() =>
      apply(initial, 1n, {
        type: "session_opened",
        sessionId: asSessionId("another-session"),
      }),
    ).toThrow(/session/u);
  });

  test("rejects events after an uncertain tool outcome", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, {
      type: "user_message_submitted",
      sessionId,
      text: "change it",
    });
    state = apply(state, 3n, { type: "run_started", sessionId });
    state = apply(state, 4n, {
      type: "tool_outcome_unknown",
      sessionId,
      callId: "call-1",
      toolName: "write_file",
      code: "UNKNOWN",
      message: "unknown",
    });

    expect(() => apply(state, 5n, { type: "run_started", sessionId })).toThrow(/terminal/u);
  });

  test("does not let a new user message erase an unresolved running tool", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, {
      type: "user_message_submitted",
      sessionId,
      text: "change it",
    });
    state = apply(state, 3n, { type: "run_started", sessionId });
    state = apply(state, 4n, {
      type: "tool_started",
      sessionId,
      callId: "call-pending",
      toolName: "write_file",
      argumentsPreview: {},
    });

    expect(() =>
      apply(state, 5n, {
        type: "user_message_submitted",
        sessionId,
        text: "erase the pending tool",
      }),
    ).toThrow(/unresolved/u);
    expect(state.tools).toEqual([
      expect.objectContaining({ callId: "call-pending", status: "running" }),
    ]);
  });

  test("bounds cumulative assistant text bytes across deltas", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, {
      type: "user_message_submitted",
      sessionId,
      text: "question",
    });
    state = apply(state, 3n, {
      type: "assistant_text_delta",
      sessionId,
      delta: "a".repeat(MAX_AGENT_RUNTIME_ASSISTANT_BYTES),
    });

    expect(() => apply(state, 4n, { type: "assistant_text_delta", sessionId, delta: "b" })).toThrow(
      /assistant text/u,
    );
    expect(state.assistantTextBytes).toBe(MAX_AGENT_RUNTIME_ASSISTANT_BYTES);
  });

  test("rejects usage totals that overflow safe integers", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, {
      type: "user_message_submitted",
      sessionId,
      text: "question",
    });
    state = apply(state, 3n, {
      type: "usage_recorded",
      sessionId,
      inputTokens: Number.MAX_SAFE_INTEGER,
      outputTokens: 0,
      totalTokens: Number.MAX_SAFE_INTEGER,
    });

    expect(() =>
      apply(state, 4n, {
        type: "usage_recorded",
        sessionId,
        inputTokens: 1,
        outputTokens: 0,
        totalTokens: 1,
      }),
    ).toThrow(/usage/u);
  });

  test("retains steering and a durable follow-up queue across event replay", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, {
      type: "user_message_submitted",
      sessionId,
      text: "first",
    });
    state = apply(state, 3n, { type: "run_started", sessionId });
    state = apply(state, 4n, { type: "steering_submitted", sessionId, text: "focus" });
    state = apply(state, 5n, { type: "follow_up_queued", sessionId, text: "second" });

    expect(state.turns[0]).toMatchObject({ steering: ["focus"] });
    expect(state.queuedFollowUps).toEqual(["second"]);

    state = apply(state, 6n, { type: "run_completed", sessionId });
    state = apply(state, 7n, { type: "follow_up_started", sessionId, text: "second" });
    expect(state.queuedFollowUps).toEqual([]);
    expect(state.turns).toHaveLength(2);
    expect(state.turns[1]).toMatchObject({ userText: "second", status: "submitted" });
  });

  test("does not dequeue a follow-up while the preceding turn is unresolved", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, { type: "user_message_submitted", sessionId, text: "first" });
    state = apply(state, 3n, { type: "run_started", sessionId });
    state = apply(state, 4n, { type: "follow_up_queued", sessionId, text: "second" });

    expect(() =>
      apply(state, 5n, { type: "follow_up_started", sessionId, text: "second" }),
    ).toThrow(/unresolved/u);
    expect(state.queuedFollowUps).toEqual(["second"]);
  });

  test("attributes usage to each activity instead of only the session total", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    const events: AgentRuntimeEvent[] = [
      { type: "session_opened", sessionId },
      { type: "user_message_submitted", sessionId, text: "first" },
      { type: "run_started", sessionId },
      { type: "turn_started", sessionId, turn: 1 },
      { type: "usage_recorded", sessionId, inputTokens: 2, outputTokens: 1, totalTokens: 3 },
      { type: "turn_completed", sessionId, turn: 1 },
      { type: "run_completed", sessionId },
      { type: "user_message_submitted", sessionId, text: "second" },
      { type: "run_started", sessionId },
      { type: "turn_started", sessionId, turn: 1 },
      { type: "usage_recorded", sessionId, inputTokens: 5, outputTokens: 3, totalTokens: 8 },
    ];
    for (const [index, event] of events.entries()) state = apply(state, BigInt(index + 1), event);

    expect(state.usage).toEqual({ inputTokens: 7, outputTokens: 4, totalTokens: 11 });
    expect(state.turns[0]?.activities[0]?.usage).toEqual({
      inputTokens: 2,
      outputTokens: 1,
      totalTokens: 3,
    });
    expect(state.turns[1]?.activities[0]?.usage).toEqual({
      inputTokens: 5,
      outputTokens: 3,
      totalTokens: 8,
    });
  });
});
