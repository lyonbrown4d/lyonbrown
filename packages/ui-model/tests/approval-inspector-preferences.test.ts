import { describe, expect, test } from "vitest";

import {
  DEFAULT_LOCALE,
  DEFAULT_THEME_ID,
  THEME_IDS,
  createInitialUiModelState,
  getApprovalDisposition,
  getInputPriority,
  getSendableApprovalDenyIntent,
  reduceUiModel,
  type RemoteSnapshot,
  type UiModelState,
} from "@lyonbrown/ui-model/internal/index";

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
  test("ignores approval requests until a fresh daemon epoch is available", () => {
    let state = createInitialUiModelState();
    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-a",
    });
    const unchanged = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "too-early", summary: "Run command" },
    });

    expect(unchanged).toBe(state);
    expect(unchanged.approval).toBeNull();
  });

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
      serverInstanceId: "epoch-a",
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
      serverInstanceId: "epoch-a",
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
      serverInstanceId: "epoch-a",
    });
  });

  test("fences a disconnect denial until the same daemon epoch is fresh again", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "approval-a", summary: "Run command" },
    });
    state = reduceUiModel(state, { type: "connectionLost", reason: "transport-closed" });

    expect(getSendableApprovalDenyIntent(state)).toBeNull();

    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-a",
    });
    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", "session-a", 1n),
    });

    expect(getSendableApprovalDenyIntent(state)).toEqual({
      requestId: "approval-a",
      reason: "disconnected",
      serverInstanceId: "epoch-a",
    });
  });

  test("clears an approval intent before a different daemon epoch becomes fresh", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "approval-a", summary: "Run command" },
    });
    state = reduceUiModel(state, { type: "approvalDenied" });
    state = reduceUiModel(state, { type: "connectionLost", reason: "transport-closed" });
    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-b",
    });
    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-b", "session-b", 1n),
    });

    expect(getApprovalDisposition(state)).toEqual({ kind: "none" });
    expect(getSendableApprovalDenyIntent(state)).toBeNull();
  });

  test("resolves an approval only when request and daemon epoch both match", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "approval-a", summary: "Run command" },
    });

    const mismatched = reduceUiModel(state, {
      type: "approvalResolved",
      requestId: "approval-a",
      serverInstanceId: "epoch-b",
    });
    expect(mismatched).toBe(state);

    state = reduceUiModel(state, {
      type: "approvalResolved",
      requestId: "approval-a",
      serverInstanceId: "epoch-a",
    });
    expect(state.approval).toBeNull();
  });

  test("does not expose a denial whose bound epoch differs from the fresh daemon", () => {
    let state = readyState();
    state = reduceUiModel(state, {
      type: "approvalRequested",
      request: { requestId: "approval-a", summary: "Run command" },
    });
    state = reduceUiModel(state, { type: "approvalDenied" });

    expect(
      getSendableApprovalDenyIntent({
        ...state,
        approval:
          state.approval === null ? null : { ...state.approval, serverInstanceId: "epoch-b" },
      }),
    ).toBeNull();

    if (state.approval === null) {
      throw new Error("expected a pending denial");
    }
    const mismatchedEpochState = {
      ...state,
      approval: { ...state.approval, serverInstanceId: "epoch-b" },
    };
    expect(
      reduceUiModel(mismatchedEpochState, {
        type: "approvalResolved",
        requestId: "approval-a",
        serverInstanceId: "epoch-b",
      }),
    ).toBe(mismatchedEpochState);
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

test("updates locale and theme preferences without changing remote identifiers", () => {
  let state = readyState();
  const remote = state.remote;
  state = reduceUiModel(state, { type: "localeChanged", locale: "zh-CN" });
  state = reduceUiModel(state, { type: "themeChanged", themeId: "carbon-mint" });

  expect(state.preferences).toEqual({ locale: "zh-CN", themeId: "carbon-mint" });
  expect(state.remote).toBe(remote);
  expect(state.remote.serverInstanceId).toBe("epoch-a");
  expect(state.remote.snapshot?.sessionId).toBe("session-a");
});
