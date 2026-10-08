import { describe, expect, test } from "vitest";

import {
  RUNTIME_JSON_MAX_STRING_BYTES,
  RUNTIME_JSON_MAX_TOTAL_STRING_BYTES,
  asSessionId,
  parseAgentRuntimeEvent,
} from "@lyonbrown/agent-domain/internal/index";

describe("runtime event boundary", () => {
  test("accepts a plain domain event and freezes nested values", () => {
    const event = parseAgentRuntimeEvent({
      type: "tool_succeeded",
      sessionId: "session-1",
      callId: "call-1",
      toolName: "read_file",
      content: "ok",
      details: { lines: 3 },
    });

    expect(event).toEqual({
      type: "tool_succeeded",
      sessionId: asSessionId("session-1"),
      callId: "call-1",
      toolName: "read_file",
      content: "ok",
      details: { lines: 3 },
    });
    expect(Object.isFrozen(event)).toBe(true);
    expect(event.type).toBe("tool_succeeded");
    if (event.type !== "tool_succeeded") throw new Error("expected tool_succeeded");
    expect(Object.isFrozen(event.details)).toBe(true);
  });

  test.each([
    { type: "steering_submitted", sessionId: "session-1", text: "focus" },
    { type: "follow_up_queued", sessionId: "session-1", text: "summarize" },
    { type: "follow_up_started", sessionId: "session-1", text: "summarize" },
  ] as const)("accepts the durable control event $type", (input) => {
    expect(parseAgentRuntimeEvent(input)).toEqual({
      ...input,
      sessionId: asSessionId("session-1"),
    });
  });

  test("accepts and freezes a bounded pre-kernel decision event", () => {
    const event = parseAgentRuntimeEvent({
      type: "decision_recorded",
      sessionId: "session-1",
      questionVersion: "system-one-v1",
      providerId: "open-jev",
      modelId: "kev-0.6b",
      modelRevision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
      intent: "code-change",
      risk: "medium",
      requiresTools: true,
      confidence: 0.8,
      probabilities: { intent: { chat: 0.2, "code-change": 0.8 } },
      action: "continue_pi",
    });

    expect(event).toMatchObject({ type: "decision_recorded", confidence: 0.8 });
    if (event.type !== "decision_recorded") throw new Error("expected decision_recorded");
    expect(Object.isFrozen(event.probabilities)).toBe(true);
  });

  test.each([
    { confidence: Number.NaN },
    { confidence: 1.1 },
    { confidence: -0.1 },
    { confidence: 0.8, intent: "unknown" },
    { confidence: 0.8, action: "grant_authority" },
  ])("rejects malformed decision data %#", (overrides) => {
    expect(() =>
      parseAgentRuntimeEvent({
        type: "decision_recorded",
        sessionId: "session-1",
        questionVersion: "system-one-v1",
        providerId: "open-jev",
        modelId: "kev-0.6b",
        modelRevision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
        intent: "code-change",
        risk: "medium",
        requiresTools: true,
        probabilities: { intent: { "code-change": 1 } },
        action: "continue_pi",
        ...overrides,
      }),
    ).toThrow();
  });

  test.each([
    { value: { type: "unknown", sessionId: "session-1" }, reason: /event type/u },
    { value: { type: "__proto__", sessionId: "session-1" }, reason: /event type/u },
    {
      value: { type: "session_opened", sessionId: "session-1", extra: true },
      reason: /unexpected/u,
    },
    {
      value: { type: "usage_recorded", sessionId: "session-1", inputTokens: -1 },
      reason: /inputTokens/u,
    },
    {
      value: Object.defineProperty({}, "type", { get: () => "session_opened" }),
      reason: /plain data/u,
    },
  ])("rejects hostile or malformed events", ({ value, reason }) => {
    expect(() => parseAgentRuntimeEvent(value)).toThrow(reason);
  });

  test("bounds individual and cumulative UTF-8 strings while copying JSON", () => {
    const base = {
      type: "tool_started",
      sessionId: "session-1",
      callId: "call-1",
      toolName: "read_file",
    } as const;
    expect(() =>
      parseAgentRuntimeEvent({
        ...base,
        argumentsPreview: { value: "界".repeat(RUNTIME_JSON_MAX_STRING_BYTES) },
      }),
    ).toThrow(/string bytes/u);

    const chunk = "a".repeat(RUNTIME_JSON_MAX_STRING_BYTES);
    const entries = Math.floor(RUNTIME_JSON_MAX_TOTAL_STRING_BYTES / chunk.length) + 1;
    expect(() =>
      parseAgentRuntimeEvent({
        ...base,
        argumentsPreview: Array.from({ length: entries }, () => chunk),
      }),
    ).toThrow(/total string bytes/u);
  });
});
