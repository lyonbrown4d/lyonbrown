import { describe, expect, test } from "vitest";

import {
  createInitialUiModelState,
  getChatSubmitAvailability,
  getDaemonMutationAvailability,
  getModelSetupCancellation,
  getSendableModelDownloadIntent,
  reduceUiModel,
  type ModelSetupPhase,
  type ModelSetupState,
  type UiModelState,
} from "../src/index.js";

function setupFor(phase: ModelSetupPhase): ModelSetupState {
  if (phase === "Required" || phase === "Ready") {
    return { operationId: null, phase };
  }
  return { operationId: "operation-a", phase };
}

function withModelPhase(phase: ModelSetupPhase): UiModelState {
  let state = createInitialUiModelState();
  state = reduceUiModel(state, {
    type: "handshakeSucceeded",
    serverInstanceId: "epoch-a",
  });
  return reduceUiModel(state, {
    type: "snapshotReceived",
    snapshot: {
      serverInstanceId: "epoch-a",
      revision: 1n,
      sessionId: "session-a",
      modelSetup: setupFor(phase),
      activities: [],
    },
  });
}

describe("model setup", () => {
  test.each<ModelSetupPhase>(["WaitingForLock", "Checking", "Downloading", "Verifying"])(
    "allows cancelling the pre-install phase %s",
    (phase) => {
      expect(getModelSetupCancellation(withModelPhase(phase))).toEqual({
        allowed: true,
        operationId: "operation-a",
      });
    },
  );

  test("reports no operation while setup is required", () => {
    expect(getModelSetupCancellation(withModelPhase("Required"))).toEqual({
      allowed: false,
      reason: "no-operation",
    });
  });

  test.each<ModelSetupPhase>(["Installing", "Activating"])(
    "reports cancellation as too late during %s",
    (phase) => {
      expect(getModelSetupCancellation(withModelPhase(phase))).toEqual({
        allowed: false,
        reason: "too-late",
      });
    },
  );

  test.each<ModelSetupPhase>(["Cancelled", "Failed"])(
    "does not cancel the terminal phase %s",
    (phase) => {
      expect(getModelSetupCancellation(withModelPhase(phase))).toEqual({
        allowed: false,
        reason: "terminal",
      });
    },
  );

  test("reports no operation once setup is ready", () => {
    expect(getModelSetupCancellation(withModelPhase("Ready"))).toEqual({
      allowed: false,
      reason: "no-operation",
    });
  });

  test("creates a download intent only from a fresh current daemon epoch", () => {
    let state = createInitialUiModelState();
    state = reduceUiModel(state, { type: "clientOpened" });
    state = reduceUiModel(state, {
      type: "modelDownloadRequested",
      operationId: "too-early",
      modelId: "qwen3-1.7b-q4_k_m",
    });
    state = reduceUiModel(state, { type: "connectRequested" });
    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-a",
    });
    state = reduceUiModel(state, {
      type: "modelDownloadRequested",
      operationId: "still-synchronizing",
      modelId: "qwen3-1.7b-q4_k_m",
    });

    expect(state.modelDownloadIntent).toBeNull();

    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: {
        serverInstanceId: "epoch-a",
        revision: 1n,
        sessionId: "session-a",
        modelSetup: { operationId: null, phase: "Required" },
        activities: [],
      },
    });
    state = reduceUiModel(state, {
      type: "modelDownloadRequested",
      operationId: "explicit-operation",
      modelId: "qwen3-1.7b-q4_k_m",
    });
    expect(state.modelDownloadIntent).toEqual({
      operationId: "explicit-operation",
      modelId: "qwen3-1.7b-q4_k_m",
      serverInstanceId: "epoch-a",
    });
    expect(getSendableModelDownloadIntent(state)).toBe(state.modelDownloadIntent);
  });

  test("clears a pending download intent when a different daemon epoch is established", () => {
    let state = withModelPhase("Required");
    state = reduceUiModel(state, {
      type: "modelDownloadRequested",
      operationId: "operation-a",
      modelId: "qwen3-1.7b-q4_k_m",
    });
    state = reduceUiModel(state, { type: "connectionLost", reason: "transport-closed" });

    expect(state.modelDownloadIntent?.serverInstanceId).toBe("epoch-a");
    expect(getSendableModelDownloadIntent(state)).toBeNull();

    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-b",
    });
    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: {
        serverInstanceId: "epoch-b",
        revision: 1n,
        sessionId: "session-b",
        modelSetup: { operationId: null, phase: "Required" },
        activities: [],
      },
    });

    expect(state.modelDownloadIntent).toBeNull();
    expect(getSendableModelDownloadIntent(state)).toBeNull();
  });

  test("makes a fenced download intent sendable again after same-epoch resynchronization", () => {
    let state = withModelPhase("Required");
    state = reduceUiModel(state, {
      type: "modelDownloadRequested",
      operationId: "operation-a",
      modelId: "qwen3-1.7b-q4_k_m",
    });
    state = reduceUiModel(state, { type: "connectionLost", reason: "transport-closed" });
    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-a",
    });

    expect(getSendableModelDownloadIntent(state)).toBeNull();

    state = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: {
        serverInstanceId: "epoch-a",
        revision: 1n,
        sessionId: "session-a",
        modelSetup: { operationId: null, phase: "Required" },
        activities: [],
      },
    });
    expect(getSendableModelDownloadIntent(state)).toEqual({
      operationId: "operation-a",
      modelId: "qwen3-1.7b-q4_k_m",
      serverInstanceId: "epoch-a",
    });
  });

  test("acknowledges a download intent only with its bound daemon epoch and operation", () => {
    let state = withModelPhase("Required");
    state = reduceUiModel(state, {
      type: "modelDownloadRequested",
      operationId: "operation-a",
      modelId: "qwen3-1.7b-q4_k_m",
    });

    const unchanged = reduceUiModel(state, {
      type: "modelDownloadIntentAcknowledged",
      operationId: "operation-a",
      serverInstanceId: "epoch-b",
    });
    expect(unchanged).toBe(state);

    const intent = state.modelDownloadIntent;
    if (intent === null) {
      throw new Error("expected the model download intent to remain pending");
    }
    expect(
      getSendableModelDownloadIntent({
        ...state,
        modelDownloadIntent: {
          ...intent,
          serverInstanceId: "epoch-b",
        },
      }),
    ).toBeNull();
    const mismatchedEpochState = {
      ...state,
      modelDownloadIntent: { ...intent, serverInstanceId: "epoch-b" },
    };
    expect(
      reduceUiModel(mismatchedEpochState, {
        type: "modelDownloadIntentAcknowledged",
        operationId: "operation-a",
        serverInstanceId: "epoch-b",
      }),
    ).toBe(mismatchedEpochState);

    state = reduceUiModel(state, {
      type: "modelDownloadIntentAcknowledged",
      operationId: "operation-a",
      serverInstanceId: "epoch-a",
    });
    expect(state.modelDownloadIntent).toBeNull();
  });

  test.each<ModelSetupPhase>([
    "Required",
    "WaitingForLock",
    "Checking",
    "Downloading",
    "Verifying",
    "Installing",
    "Activating",
    "Cancelled",
    "Failed",
  ])("disables chat but preserves daemon controls during %s", (phase) => {
    const state = withModelPhase(phase);
    expect(getDaemonMutationAvailability(state)).toEqual({ enabled: true });
    expect(getChatSubmitAvailability(state)).toEqual({
      enabled: false,
      reason: "model-setup-required",
    });
  });

  test("enables chat and daemon controls when setup is ready", () => {
    const state = withModelPhase("Ready");
    expect(getDaemonMutationAvailability(state)).toEqual({ enabled: true });
    expect(getChatSubmitAvailability(state)).toEqual({ enabled: true });
  });
});
