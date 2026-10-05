import { describe, expect, it } from "vitest";

import { recoverToolCalls, type StoredEvent } from "../src/index.ts";

function event(revision: bigint, type: string, payload: unknown): StoredEvent {
  return {
    eventId: `event-${revision.toString()}`,
    sessionId: "session-a",
    revision,
    type,
    occurredAt: `2026-01-01T00:00:0${revision.toString()}.000Z`,
    payload,
  };
}

describe("tool-call recovery", () => {
  it("marks started calls without a terminal event outcome-unknown and never invents a retry", () => {
    const recovered = recoverToolCalls([
      event(1n, "ToolStarted", { toolCallId: "call-1", toolName: "write_file" }),
    ]);

    expect(recovered).toEqual([
      expect.objectContaining({ toolCallId: "call-1", status: "outcome-unknown" }),
    ]);
    expect(recovered[0]).not.toHaveProperty("retry");
  });

  it.each([
    ["ToolSucceeded", "completed"],
    ["ToolFailed", "failed"],
    ["ToolCancelled", "cancelled"],
  ] as const)("preserves terminal %s state as %s", (terminalType, expectedStatus) => {
    const recovered = recoverToolCalls([
      event(1n, "ToolStarted", { toolCallId: "call-1", toolName: "shell" }),
      event(2n, terminalType, { toolCallId: "call-1", summary: "recorded" }),
    ]);

    expect(recovered).toEqual([
      expect.objectContaining({
        toolCallId: "call-1",
        status: expectedStatus,
        terminalEventId: "event-2",
      }),
    ]);
  });

  it("uses revision order and ignores malformed or orphan terminal events", () => {
    const recovered = recoverToolCalls([
      event(3n, "ToolSucceeded", { toolCallId: "call-1" }),
      event(1n, "ToolSucceeded", { toolCallId: "orphan" }),
      event(2n, "ToolStarted", { toolCallId: "call-1", toolName: "read_file" }),
      event(4n, "ToolStarted", { toolName: "missing-id" }),
    ]);

    expect(recovered).toEqual([
      expect.objectContaining({ toolCallId: "call-1", status: "completed" }),
    ]);
  });
});
