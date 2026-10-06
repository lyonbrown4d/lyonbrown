import {
  DEFAULT_LOCAL_MODEL,
  type LocalModelPhase,
  type LocalModelStatus,
} from "@lyonbrown/local-model";
import {
  ModelRecommendationReason,
  ModelSetupAction,
  ModelSetupPhase,
  RuntimePhase,
} from "@lyonbrown/protocol";
import { describe, expect, it } from "vitest";

import {
  projectModelSetup,
  projectSetupRuntimePhase,
} from "@lyonbrown/agentd/internal/model-setup-projection";

function status(
  phase: LocalModelPhase,
  overrides: Partial<LocalModelStatus> = {},
): LocalModelStatus {
  return {
    model: DEFAULT_LOCAL_MODEL,
    phase,
    operationId: null,
    downloadedBytes: 0,
    totalBytes: null,
    failure: null,
    selectionReason: null,
    allowedActions: [],
    ...overrides,
  };
}

describe("model setup projection", () => {
  it.each<[LocalModelPhase, ModelSetupPhase]>([
    ["setup-required", ModelSetupPhase.MODEL_SETUP_PHASE_REQUIRED],
    ["waiting-for-lock", ModelSetupPhase.MODEL_SETUP_PHASE_WAITING_FOR_LOCK],
    ["checking", ModelSetupPhase.MODEL_SETUP_PHASE_CHECKING],
    ["downloading", ModelSetupPhase.MODEL_SETUP_PHASE_DOWNLOADING],
    ["verifying", ModelSetupPhase.MODEL_SETUP_PHASE_VERIFYING],
    ["installing", ModelSetupPhase.MODEL_SETUP_PHASE_INSTALLING],
    ["ready", ModelSetupPhase.MODEL_SETUP_PHASE_READY],
    ["cancelled", ModelSetupPhase.MODEL_SETUP_PHASE_CANCELLED],
    ["failed", ModelSetupPhase.MODEL_SETUP_PHASE_FAILED],
  ])("maps local phase %s", (phase, expected) => {
    expect(projectModelSetup(status(phase), "inactive").phase).toBe(expected);
  });

  it("projects managed metadata, progress, actions, and a redacted setup failure", () => {
    const snapshot = projectModelSetup(
      status("failed", {
        operationId: "operation-1",
        downloadedBytes: 512,
        totalBytes: 1_024,
        failure: { code: "download_failed", retryable: true },
        allowedActions: ["download", "cancel", "retry"],
      }),
      "inactive",
    );

    expect(snapshot).toMatchObject({
      operationId: "operation-1",
      downloadedBytes: 512n,
      totalBytes: 1_024n,
      managedModel: {
        id: DEFAULT_LOCAL_MODEL.id,
        expectedDownloadBytes: BigInt(DEFAULT_LOCAL_MODEL.expectedDownloadBytes),
        maxBytes: BigInt(DEFAULT_LOCAL_MODEL.maxBytes),
        minAvailableMemoryBytes: BigInt(DEFAULT_LOCAL_MODEL.minimumMemoryBytes),
        sha256: DEFAULT_LOCAL_MODEL.sha256,
      },
      failure: {
        code: "download_failed",
        message: "local model setup failed",
        retryable: true,
      },
      allowedActions: [
        ModelSetupAction.MODEL_SETUP_ACTION_DOWNLOAD,
        ModelSetupAction.MODEL_SETUP_ACTION_CANCEL,
        ModelSetupAction.MODEL_SETUP_ACTION_RETRY,
      ],
    });
  });

  it.each([
    [
      "installed-preferred",
      ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_INSTALLED_PREFERRED,
    ],
    [
      "hardware-compatible",
      ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_HARDWARE_COMPATIBLE,
    ],
    [
      "conservative-fallback",
      ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_CONSERVATIVE_FALLBACK,
    ],
    [null, undefined],
  ] as const)("projects the %s model recommendation reason", (selectionReason, expected) => {
    const modelStatus = {
      ...status("setup-required"),
      selectionReason,
    };

    expect(projectModelSetup(modelStatus, "inactive").managedModel).toMatchObject({
      displayName: DEFAULT_LOCAL_MODEL.displayName,
      recommendationReason: expected,
    });
  });

  it("overrides ready status while activating and exposes a retryable activation failure", () => {
    expect(projectModelSetup(status("ready"), "activating").phase).toBe(
      ModelSetupPhase.MODEL_SETUP_PHASE_ACTIVATING,
    );
    expect(projectModelSetup(status("ready"), "failed")).toMatchObject({
      phase: ModelSetupPhase.MODEL_SETUP_PHASE_FAILED,
      failure: {
        code: "activation_failed",
        message: "local model activation failed",
        retryable: true,
      },
      allowedActions: [ModelSetupAction.MODEL_SETUP_ACTION_RETRY],
    });
  });

  it.each([
    [status("failed"), "inactive" as const, RuntimePhase.RUNTIME_PHASE_FAILED_RECOVERABLE],
    [status("ready"), "failed" as const, RuntimePhase.RUNTIME_PHASE_FAILED_RECOVERABLE],
    [status("checking"), "inactive" as const, RuntimePhase.RUNTIME_PHASE_INITIALIZING],
    [status("ready"), "activating" as const, RuntimePhase.RUNTIME_PHASE_INITIALIZING],
    [
      status("setup-required"),
      "inactive" as const,
      RuntimePhase.RUNTIME_PHASE_MODEL_SETUP_REQUIRED,
    ],
  ])("projects the daemon runtime phase", (modelStatus, activation, expected) => {
    expect(projectSetupRuntimePhase(modelStatus, activation)).toBe(expected);
  });
});
