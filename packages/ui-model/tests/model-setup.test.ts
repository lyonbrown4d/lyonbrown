import { describe, expect, test } from "vitest";

import {
  createInitialUiModelState,
  getModelSetupCancellation,
  getMutationAvailability,
  reduceUiModel,
  type ModelSetupPhase,
  type UiModelState,
} from "../src/index.js";

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
      modelSetup: { operationId: "operation-a", phase },
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

  test.each<ModelSetupPhase>(["Installing", "Activating"])(
    "reports cancellation as too late during %s",
    (phase) => {
      expect(getModelSetupCancellation(withModelPhase(phase))).toEqual({
        allowed: false,
        reason: "too-late",
      });
    },
  );

  test.each<ModelSetupPhase>(["Ready", "Cancelled", "Failed"])(
    "does not cancel the terminal phase %s",
    (phase) => {
      expect(getModelSetupCancellation(withModelPhase(phase))).toEqual({
        allowed: false,
        reason: "terminal",
      });
    },
  );

  test("never creates a download intent while opening or connecting", () => {
    let state = createInitialUiModelState();
    state = reduceUiModel(state, { type: "clientOpened" });
    state = reduceUiModel(state, { type: "connectRequested" });
    state = reduceUiModel(state, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-a",
    });

    expect(state.modelDownloadIntent).toBeNull();

    state = reduceUiModel(state, {
      type: "modelDownloadRequested",
      operationId: "explicit-operation",
      modelId: "qwen3-1.7b-q4_k_m",
    });
    expect(state.modelDownloadIntent).toEqual({
      operationId: "explicit-operation",
      modelId: "qwen3-1.7b-q4_k_m",
    });
  });

  test("disables chat while model setup is required", () => {
    expect(getMutationAvailability(withModelPhase("Downloading"))).toEqual({
      enabled: false,
      reason: "model-setup-required",
    });
    expect(getMutationAvailability(withModelPhase("Ready"))).toEqual({ enabled: true });
  });
});
