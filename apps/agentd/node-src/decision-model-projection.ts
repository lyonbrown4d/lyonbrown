import { DecisionModelPhase, DecisionModelSnapshot, ModelSetupAction } from "@lyonbrown/protocol";

import type { DecisionModelStatus } from "@lyonbrown/agentd/internal/decision-model-state";

const PHASES = {
  unavailable: DecisionModelPhase.DECISION_MODEL_PHASE_UNAVAILABLE,
  checking: DecisionModelPhase.DECISION_MODEL_PHASE_CHECKING,
  required: DecisionModelPhase.DECISION_MODEL_PHASE_REQUIRED,
  "waiting-for-lock": DecisionModelPhase.DECISION_MODEL_PHASE_WAITING_FOR_LOCK,
  downloading: DecisionModelPhase.DECISION_MODEL_PHASE_DOWNLOADING,
  verifying: DecisionModelPhase.DECISION_MODEL_PHASE_VERIFYING,
  installing: DecisionModelPhase.DECISION_MODEL_PHASE_INSTALLING,
  configured: DecisionModelPhase.DECISION_MODEL_PHASE_CONFIGURED,
  cancelled: DecisionModelPhase.DECISION_MODEL_PHASE_CANCELLED,
  failed: DecisionModelPhase.DECISION_MODEL_PHASE_FAILED,
} satisfies Record<DecisionModelStatus["phase"], DecisionModelPhase>;

const ACTIONS = {
  download: ModelSetupAction.MODEL_SETUP_ACTION_DOWNLOAD,
  cancel: ModelSetupAction.MODEL_SETUP_ACTION_CANCEL,
  retry: ModelSetupAction.MODEL_SETUP_ACTION_RETRY,
} as const;

export function projectDecisionModel(
  status: DecisionModelStatus | undefined,
): DecisionModelSnapshot | undefined {
  if (status === undefined) return undefined;
  return DecisionModelSnapshot.create({
    ...status,
    phase: PHASES[status.phase],
    downloadedBytes: BigInt(status.downloadedBytes),
    totalBytes: BigInt(status.totalBytes),
    operationId: status.operationId ?? undefined,
    failure: status.failure ?? undefined,
    allowedActions: status.allowedActions.map((action) => ACTIONS[action]),
  });
}
