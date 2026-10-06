import { describe, expect, it } from "vitest";

import {
  asRevision,
  asSessionId,
  createInitialAgentRuntimeState,
  parseAgentRuntimeEvent,
  reduceAgentRuntimeEvent,
  type AgentRuntimeEvent,
  type AgentRuntimeState,
} from "@lyonbrown/agent-domain/internal/index";

const sessionId = asSessionId("approval-session");

function apply(state: AgentRuntimeState, revision: bigint, event: AgentRuntimeEvent) {
  return reduceAgentRuntimeEvent(state, { revision: asRevision(revision), event });
}

describe("durable approval state", () => {
  it("projects one correlated pending approval and clears it after resolution", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, { type: "user_message_submitted", sessionId, text: "write" });
    state = apply(state, 3n, { type: "run_started", sessionId });
    state = apply(state, 4n, { type: "turn_started", sessionId, turn: 1 });
    state = apply(
      state,
      5n,
      parseAgentRuntimeEvent({
        type: "approval_requested",
        sessionId,
        requestId: "call-1",
        toolName: "write",
        risk: "medium",
        permissions: ["workspace.write"],
        argumentsPreview: { path: "notes.txt" },
      }),
    );

    expect(state.pendingApproval).toEqual({
      requestId: "call-1",
      toolName: "write",
      risk: "medium",
      permissions: ["workspace.write"],
      argumentsPreview: { path: "notes.txt" },
    });

    state = apply(state, 6n, {
      type: "approval_resolved",
      sessionId,
      requestId: "call-1",
      decision: "allow-for-session",
    });
    expect(state.pendingApproval).toBeUndefined();
    expect(state.approvalPolicy).toEqual({ allowedTools: ["write"], denyRemaining: false });
  });

  it("rejects a mismatched approval resolution", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, { type: "user_message_submitted", sessionId, text: "write" });
    state = apply(state, 3n, { type: "run_started", sessionId });
    expect(() =>
      apply(state, 4n, {
        type: "approval_resolved",
        sessionId,
        requestId: "missing",
        decision: "deny-once",
      }),
    ).toThrow(/approval/u);
  });

  it("reconstructs deny-remaining policy from the durable event stream", () => {
    let state = createInitialAgentRuntimeState(sessionId);
    state = apply(state, 1n, { type: "session_opened", sessionId });
    state = apply(state, 2n, { type: "user_message_submitted", sessionId, text: "write" });
    state = apply(state, 3n, { type: "run_started", sessionId });
    state = apply(state, 4n, {
      type: "approval_requested",
      sessionId,
      requestId: "call-1",
      toolName: "write",
      risk: "medium",
      permissions: ["workspace.write"],
      argumentsPreview: { path: "notes.txt" },
    });

    state = apply(state, 5n, {
      type: "approval_resolved",
      sessionId,
      requestId: "call-1",
      decision: "deny-remaining",
    });

    expect(state.approvalPolicy).toEqual({ allowedTools: [], denyRemaining: true });
  });
});
