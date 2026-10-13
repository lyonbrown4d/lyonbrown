import { DecisionModelPhase, DecisionModelSnapshot, ModelSetupAction } from "@lyonbrown/protocol";
import { describe, expect, it } from "vitest";

import { mapDecisionModel } from "@lyonbrown/agent-client/internal/snapshot-decision";

function snapshot() {
  return DecisionModelSnapshot.create({
    providerId: "open-jev",
    modelId: "kev-0.6b",
    modelRevision: "a".repeat(40),
    phase: DecisionModelPhase.DECISION_MODEL_PHASE_REQUIRED,
    downloadedBytes: 0n,
    totalBytes: 100n,
    allowedActions: [ModelSetupAction.MODEL_SETUP_ACTION_DOWNLOAD],
    advisoryOnly: true,
  });
}

describe("decision model wire boundary", () => {
  it("maps installation metadata independently from chat state", () => {
    expect(mapDecisionModel(snapshot())).toMatchObject({
      phase: "required",
      advisoryOnly: true,
      allowedActions: ["download"],
      operationId: null,
      failure: null,
    });
    expect(mapDecisionModel(undefined)).toBeNull();
  });

  it("accepts cancellable progress with a current operation and sanitized failure fields", () => {
    expect(
      mapDecisionModel(
        DecisionModelSnapshot.create({
          ...snapshot(),
          phase: DecisionModelPhase.DECISION_MODEL_PHASE_DOWNLOADING,
          operationId: "op",
          downloadedBytes: 30n,
          allowedActions: [ModelSetupAction.MODEL_SETUP_ACTION_CANCEL],
          failure: { code: "setup_failed", message: "Setup failed", retryable: true },
        }),
      ),
    ).toMatchObject({
      phase: "downloading",
      operationId: "op",
      downloadedBytes: 30n,
      failure: { code: "setup_failed" },
    });
  });

  it.each([
    { phase: DecisionModelPhase.DECISION_MODEL_PHASE_UNSPECIFIED },
    { advisoryOnly: false },
    { modelRevision: "main" },
    { downloadedBytes: 101n },
    { totalBytes: 0n },
    { allowedActions: [ModelSetupAction.MODEL_SETUP_ACTION_CANCEL] },
    { phase: DecisionModelPhase.DECISION_MODEL_PHASE_CONFIGURED },
    { allowedActions: [ModelSetupAction.MODEL_SETUP_ACTION_RETRY] },
    { providerId: "bad\u001bprovider" },
  ])("rejects malformed or contradictory decision state %#", (changes) => {
    expect(() =>
      mapDecisionModel(DecisionModelSnapshot.create({ ...snapshot(), ...changes })),
    ).toThrow();
  });
});
