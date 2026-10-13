import { DecisionModelPhase, type DecisionModelSnapshot } from "@lyonbrown/protocol";

import { clientError } from "@lyonbrown/agent-client/internal/errors";
import { modelAction } from "@lyonbrown/agent-client/internal/snapshot-scalars";
import type { DecisionModelDto } from "@lyonbrown/agent-client/internal/types";
import {
  protocolDisplayName,
  protocolId,
  protocolText,
  requireNonNegativeRevision,
} from "@lyonbrown/agent-client/internal/validation";

const PHASES: Partial<Record<DecisionModelPhase, DecisionModelDto["phase"]>> = {
  [DecisionModelPhase.DECISION_MODEL_PHASE_UNAVAILABLE]: "unavailable",
  [DecisionModelPhase.DECISION_MODEL_PHASE_CHECKING]: "checking",
  [DecisionModelPhase.DECISION_MODEL_PHASE_REQUIRED]: "required",
  [DecisionModelPhase.DECISION_MODEL_PHASE_WAITING_FOR_LOCK]: "waiting-for-lock",
  [DecisionModelPhase.DECISION_MODEL_PHASE_DOWNLOADING]: "downloading",
  [DecisionModelPhase.DECISION_MODEL_PHASE_VERIFYING]: "verifying",
  [DecisionModelPhase.DECISION_MODEL_PHASE_INSTALLING]: "installing",
  [DecisionModelPhase.DECISION_MODEL_PHASE_CONFIGURED]: "configured",
  [DecisionModelPhase.DECISION_MODEL_PHASE_CANCELLED]: "cancelled",
  [DecisionModelPhase.DECISION_MODEL_PHASE_FAILED]: "failed",
};

export function mapDecisionModel(
  value: DecisionModelSnapshot | undefined,
): DecisionModelDto | null {
  if (value === undefined) return null;
  const phase = PHASES[value.phase];
  const downloadedBytes = requireNonNegativeRevision(
    "decision.downloaded_bytes",
    value.downloadedBytes,
  );
  const totalBytes = requireNonNegativeRevision("decision.total_bytes", value.totalBytes);
  const allowedActions = value.allowedActions.map(modelAction);
  const operationId =
    value.operationId === undefined ? null : protocolId("decision.operation_id", value.operationId);
  if (
    phase === undefined ||
    !value.advisoryOnly ||
    totalBytes === 0n ||
    downloadedBytes > totalBytes ||
    !/^[a-f0-9]{40}$/u.test(value.modelRevision) ||
    (allowedActions.includes("cancel") &&
      (operationId === null ||
        !["checking", "waiting-for-lock", "downloading", "verifying"].includes(phase)))
  ) {
    throw clientError("protocol", "INVALID_SNAPSHOT", "decision model snapshot is invalid");
  }
  if (allowedActions.includes("download") && phase !== "required")
    throw clientError("protocol", "INVALID_SNAPSHOT", "decision download action is invalid");
  if (allowedActions.includes("retry") && phase !== "failed" && phase !== "cancelled")
    throw clientError("protocol", "INVALID_SNAPSHOT", "decision retry action is invalid");
  return {
    providerId: protocolDisplayName("decision.provider_id", value.providerId),
    modelId: protocolDisplayName("decision.model_id", value.modelId),
    modelRevision: value.modelRevision,
    advisoryOnly: true,
    phase,
    operationId,
    downloadedBytes,
    totalBytes,
    allowedActions,
    failure:
      value.failure === undefined
        ? null
        : {
            code: protocolId("decision.failure.code", value.failure.code),
            message: protocolText("decision.failure.message", value.failure.message),
            retryable: value.failure.retryable,
          },
  };
}
