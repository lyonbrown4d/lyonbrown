import type { LocalModelStatus } from "@lyonbrown/local-model";
import {
  ManagedModel,
  ModelSetupAction,
  ModelSetupFailure,
  ModelSetupPhase,
  ModelSetupSnapshot,
  ModelSetupSource,
  RuntimePhase,
} from "@lyonbrown/protocol";

export type ActivationPhase = "inactive" | "activating" | "failed";

export function projectModelSetup(
  status: LocalModelStatus,
  activation: ActivationPhase,
): ModelSetupSnapshot {
  return ModelSetupSnapshot.create({
    source: ModelSetupSource.MODEL_SETUP_SOURCE_MANAGED_PRESET,
    phase: activationPhase(status, activation),
    managedModel: ManagedModel.create({
      id: status.model.id,
      expectedDownloadBytes: BigInt(status.model.expectedDownloadBytes),
      maxBytes: BigInt(status.model.maxBytes),
      minAvailableMemoryBytes: BigInt(status.model.minimumMemoryBytes),
      sha256: status.model.sha256,
    }),
    operationId: status.operationId ?? undefined,
    downloadedBytes: BigInt(status.downloadedBytes),
    totalBytes: status.totalBytes === null ? undefined : BigInt(status.totalBytes),
    failure:
      activation === "failed"
        ? ModelSetupFailure.create({
            code: "activation_failed",
            message: "local model activation failed",
            retryable: true,
          })
        : status.failure === null
          ? undefined
          : ModelSetupFailure.create({
              code: status.failure.code,
              message: "local model setup failed",
              retryable: status.failure.retryable,
            }),
    allowedActions:
      activation === "failed"
        ? [ModelSetupAction.MODEL_SETUP_ACTION_RETRY]
        : status.allowedActions.map(action),
  });
}

export function projectSetupRuntimePhase(
  status: LocalModelStatus,
  activation: ActivationPhase,
): RuntimePhase {
  if (activation === "failed" || status.phase === "failed") {
    return RuntimePhase.RUNTIME_PHASE_FAILED_RECOVERABLE;
  }
  if (activation === "activating" || status.phase === "checking" || status.phase === "ready") {
    return RuntimePhase.RUNTIME_PHASE_INITIALIZING;
  }
  return RuntimePhase.RUNTIME_PHASE_MODEL_SETUP_REQUIRED;
}

function activationPhase(status: LocalModelStatus, activation: ActivationPhase): ModelSetupPhase {
  if (activation === "activating") return ModelSetupPhase.MODEL_SETUP_PHASE_ACTIVATING;
  if (activation === "failed") return ModelSetupPhase.MODEL_SETUP_PHASE_FAILED;
  switch (status.phase) {
    case "setup-required":
      return ModelSetupPhase.MODEL_SETUP_PHASE_REQUIRED;
    case "waiting-for-lock":
      return ModelSetupPhase.MODEL_SETUP_PHASE_WAITING_FOR_LOCK;
    case "checking":
      return ModelSetupPhase.MODEL_SETUP_PHASE_CHECKING;
    case "downloading":
      return ModelSetupPhase.MODEL_SETUP_PHASE_DOWNLOADING;
    case "verifying":
      return ModelSetupPhase.MODEL_SETUP_PHASE_VERIFYING;
    case "installing":
      return ModelSetupPhase.MODEL_SETUP_PHASE_INSTALLING;
    case "ready":
      return ModelSetupPhase.MODEL_SETUP_PHASE_READY;
    case "cancelled":
      return ModelSetupPhase.MODEL_SETUP_PHASE_CANCELLED;
    case "failed":
      return ModelSetupPhase.MODEL_SETUP_PHASE_FAILED;
  }
}

function action(value: LocalModelStatus["allowedActions"][number]): ModelSetupAction {
  switch (value) {
    case "download":
      return ModelSetupAction.MODEL_SETUP_ACTION_DOWNLOAD;
    case "cancel":
      return ModelSetupAction.MODEL_SETUP_ACTION_CANCEL;
    case "retry":
      return ModelSetupAction.MODEL_SETUP_ACTION_RETRY;
  }
}
