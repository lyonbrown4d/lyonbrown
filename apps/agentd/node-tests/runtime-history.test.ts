import {
  asSessionId,
  asRevision,
  createInitialAgentRuntimeState,
  reduceAgentRuntimeEvent,
  type AgentRuntimeEvent,
  type AgentRuntimeState,
} from "@lyonbrown/agent-domain";
import { describe, expect, it } from "vitest";

import { runtimeHistory } from "@lyonbrown/agentd/internal/runtime-history";

const sessionId = asSessionId("history-session");
type SessionlessEvent = AgentRuntimeEvent extends infer Event
  ? Event extends AgentRuntimeEvent
    ? Omit<Event, "sessionId">
    : never
  : never;

function stateFrom(events: readonly SessionlessEvent[]): AgentRuntimeState {
  let state = createInitialAgentRuntimeState(sessionId);
  for (const [index, event] of events.entries()) {
    state = reduceAgentRuntimeEvent(state, {
      revision: asRevision(BigInt(index + 1)),
      event: { ...event, sessionId } as AgentRuntimeEvent,
    });
  }
  return state;
}

describe("runtime history projection", () => {
  it("reconstructs completed assistant tool calls and results", () => {
    const state = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "read it" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      {
        type: "tool_started",
        callId: "call-1",
        toolName: "read",
        argumentsPreview: { path: "notes.txt" },
      },
      {
        type: "tool_succeeded",
        callId: "call-1",
        toolName: "read",
        content: "hello",
      },
      { type: "turn_completed", turn: 1 },
      { type: "turn_started", turn: 2 },
      { type: "assistant_text_delta", delta: "summary" },
      { type: "turn_completed", turn: 2 },
      { type: "run_completed" },
    ]);

    expect(runtimeHistory(state)).toEqual([
      { role: "user", content: "read it" },
      {
        role: "assistant",
        content: "",
        toolCalls: [{ callId: "call-1", name: "read", arguments: { path: "notes.txt" } }],
      },
      {
        role: "tool_result",
        callId: "call-1",
        name: "read",
        content: "hello",
        isError: false,
      },
      { role: "assistant", content: "summary", toolCalls: [] },
    ]);
  });

  it("refuses ambiguous steering and unresolved work", () => {
    const steered = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "start" },
      { type: "run_started" },
      { type: "steering_submitted", text: "change direction" },
      { type: "run_completed" },
    ]);
    expect(runtimeHistory(steered)).toBeUndefined();

    const running = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "start" },
      { type: "run_started" },
    ]);
    expect(runtimeHistory(running)).toBeUndefined();
  });

  it("refuses pre-execution tool failures whose original arguments are unavailable", () => {
    const denied = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "write it" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      {
        type: "approval_requested",
        requestId: "call-denied",
        toolName: "write",
        risk: "medium",
        permissions: ["workspace.write"],
        argumentsPreview: { path: "notes.txt", content: "secret" },
      },
      { type: "approval_resolved", requestId: "call-denied", decision: "deny-once" },
      {
        type: "tool_failed",
        callId: "call-denied",
        toolName: "write",
        code: "TOOL_DENIED",
        message: "Tool execution denied",
      },
      { type: "turn_completed", turn: 1 },
      { type: "run_completed" },
    ]);

    expect(runtimeHistory(denied)).toBeUndefined();
  });

  it("uses the Kernel JSON budget for escaped history before resuming", () => {
    const events: SessionlessEvent[] = [{ type: "session_opened" }];
    for (let turn = 0; turn < 4; turn += 1) {
      events.push(
        { type: "user_message_submitted", text: "\\".repeat(64_000) },
        { type: "run_started" },
        { type: "run_completed" },
      );
    }

    expect(runtimeHistory(stateFrom(events))).toBeUndefined();
  });
});
