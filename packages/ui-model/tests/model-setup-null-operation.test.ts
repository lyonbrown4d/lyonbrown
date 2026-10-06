import { describe, expect, test } from "vitest";

import {
  createInitialUiModelState,
  getChatSubmitAvailability,
  getModelSetupCancellation,
  reduceUiModel,
  type ModelSetupPhase,
  type ModelSetupState,
  type UiModelState,
} from "@lyonbrown/ui-model/internal/index";

function withModelSetup(modelSetup: ModelSetupState): UiModelState {
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
      modelSetup,
      activities: [],
    },
  });
}

describe("model setup without an operation id", () => {
  test.each<ModelSetupPhase>(["Checking", "Downloading", "Verifying", "WaitingForLock"])(
    "does not expose cancellable setup phase %s",
    (phase) => {
      const state = withModelSetup({ phase, operationId: null });

      expect(getModelSetupCancellation(state)).toEqual({
        allowed: false,
        reason: "no-operation",
      });
      expect(getChatSubmitAvailability(state)).toEqual({
        enabled: false,
        reason: "model-setup-required",
      });
    },
  );

  test.each<ModelSetupPhase>(["Activating", "Failed"])("preserves non-ready phase %s", (phase) => {
    const state = withModelSetup({ phase, operationId: null });

    expect(getChatSubmitAvailability(state)).toEqual({
      enabled: false,
      reason: "model-setup-required",
    });
    expect(getModelSetupCancellation(state)).toEqual({
      allowed: false,
      reason: phase === "Activating" ? "too-late" : "terminal",
    });
  });
});
