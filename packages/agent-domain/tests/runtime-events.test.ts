import { describe, expect, test } from "vitest";

import {
  RUNTIME_JSON_MAX_STRING_BYTES,
  RUNTIME_JSON_MAX_TOTAL_STRING_BYTES,
  asSessionId,
  parseAgentRuntimeEvent,
} from "../src/index.js";

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
