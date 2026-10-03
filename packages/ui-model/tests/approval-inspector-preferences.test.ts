import { describe, expect, test } from "vitest";

import {
  DEFAULT_LOCALE,
  DEFAULT_THEME_ID,
  THEME_IDS,
  createInitialUiModelState,
  getApprovalDisposition,
  getInputPriority,
  reduceUiModel,
  type RemoteSnapshot,
  type UiModelState,
} from "../src/index.js";

function snapshot(epoch: string, sessionId: string, revision: bigint): RemoteSnapshot {
  return {
    serverInstanceId: epoch,
    revision,
    sessionId,
    modelSetup: null,
    activities: [{ turnId: "turn-a", stepId: "step-a", kind: "tool" }],
  };
}

function readyState(): UiModelState {
  let state = createInitialUiModelState();
  state = reduceUiModel(state, { type: "handshakeSucceeded", serverInstanceId: "epoch-a" });
  return reduceUiModel(state, {
    type: "snapshotReceived",
    snapshot: snapshot("epoch-a", "session-a", 1n),
  });
}

describe("approval fail-closed behavior", () => {
  test.each([
    ["approvalTimedOut", "timeout"],
    ["approvalErrored", "error"],
    ["approvalFailed", "failure"],
    ["connectionLost", "disconnected"],
  ] as const)("turns %s into a deny intent", (type, reason) => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "approval-a", summary: "Run command" },
    });
    state = reduceUiModel(
      state,
      type === "connectionLost" ? { type, reason: "transport-closed" } : { type },
    );

    expect(getApprovalDisposition(state)).toEqual({
      kind: "deny-intent",
      requestId: "approval-a",
      reason,
    });
  });

  test("approval input outranks model setup and the composer", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "approval-a", summary: "Read file" },
    });

    expect(getInputPriority(state)).toBe("approval");
    expect(getApprovalDisposition(state)).toEqual({
      kind: "decision-required",
      requestId: "approval-a",
    });
  });

  test("model setup input outranks the composer when no approval is active", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: {
        ...snapshot("epoch-a", "session-a", 2n),
        modelSetup: { operationId: "operation-a", phase: "Checking" },
      },
    });

    expect(getInputPriority(state)).toBe("model-setup");
  });

  test("an explicit denial is represented as a deny intent", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "approval-a", summary: "Write file" },
    });
    state = reduceUiModel(state, { type: "approvalDenied" });

    expect(getApprovalDisposition(state)).toEqual({
      kind: "deny-intent",
      requestId: "approval-a",
      reason: "user",
    });
  });
});

describe("activity inspector", () => {
  test("keeps a stable selection in one session and clears it when the epoch changes", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "activitySelected",
      selection: { turnId: "turn-a", stepId: "step-a" },
    });
    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", "session-a", 2n),
    });
    expect(state.inspector.selection).toEqual({ turnId: "turn-a", stepId: "step-a" });

    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-b",
    });
    expect(state.inspector.selection).toBeNull();
  });

  test("clears selection when the session changes and updates pure view preferences", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "activitySelected",
      selection: { turnId: "turn-a", stepId: "step-a" },
    });
    state = reduceUiModel(state, { type: "followLatestChanged", value: false });
    state = reduceUiModel(state, { type: "narrowViewModeChanged", mode: "activity" });
    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", "session-b", 2n),
    });

    expect(state.inspector).toEqual({
      selection: null,
      followLatest: false,
      narrowViewMode: "activity",
    });
  });
});

test("defaults to English, hearth teal, and the six documented theme IDs", () => {
  const state = createInitialUiModelState();

  expect(DEFAULT_LOCALE).toBe("en-US");
  expect(DEFAULT_THEME_ID).toBe("hearth-teal");
  expect(THEME_IDS).toEqual([
    "carbon-mint",
    "ink-violet",
    "slate-blue",
    "hearth-teal",
    "sandstone-espresso",
    "graphite-coral",
  ]);
  expect(state.preferences).toEqual({ locale: "en-US", themeId: "hearth-teal" });
});
