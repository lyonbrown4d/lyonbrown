import { BUNDLED_DECISION_MODEL } from "@lyonbrown/decision-engine/composition";

import type { RuntimeResult } from "@lyonbrown/agentd/internal/runtime-host";

export type DecisionModelPhase =
  | "unavailable"
  | "checking"
  | "required"
  | "waiting-for-lock"
  | "downloading"
  | "verifying"
  | "installing"
  | "configured"
  | "cancelled"
  | "failed";

export interface DecisionModelStatus {
  readonly phase: DecisionModelPhase;
  readonly providerId: string;
  readonly modelId: string;
  readonly modelRevision: string;
  readonly advisoryOnly: true;
  readonly operationId: string | null;
  readonly downloadedBytes: number;
  readonly totalBytes: number;
  readonly failure: {
    readonly code: string;
    readonly message: string;
    readonly retryable: boolean;
  } | null;
  readonly allowedActions: readonly ("download" | "cancel" | "retry")[];
}

export interface DecisionModelControlPort {
  status(): DecisionModelStatus;
  subscribe(listener: (status: DecisionModelStatus) => void): () => void;
  start(modelId: string): RuntimeResult;
  cancel(operationId: string): RuntimeResult;
}

export const DECISION_MODEL_TOTAL_BYTES = BUNDLED_DECISION_MODEL.files.reduce(
  (sum, file) => sum + file.maxBytes,
  0,
);

export function initialDecisionModelStatus(phase: DecisionModelPhase): DecisionModelStatus {
  return Object.freeze({
    phase,
    providerId: "open-jev",
    modelId: BUNDLED_DECISION_MODEL.modelId,
    modelRevision: BUNDLED_DECISION_MODEL.revision,
    advisoryOnly: true,
    operationId: null,
    downloadedBytes: 0,
    totalBytes: DECISION_MODEL_TOTAL_BYTES,
    failure: null,
    allowedActions: Object.freeze([]),
  });
}
