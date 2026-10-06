import { descriptorFor } from "../public-model.ts";
import type { LocalModelSelectionReason, LocalModelStatus } from "../types.ts";
import type { ModelPreset } from "./contracts.ts";

export const statusFor = (
  preset: ModelPreset,
  selectionReason: LocalModelSelectionReason | null,
  phase: LocalModelStatus["phase"],
  operationId: string | null = null,
  downloadedBytes = 0,
  totalBytes: number | null = null,
  failure: LocalModelStatus["failure"] = null,
): LocalModelStatus => ({
  model: descriptorFor(preset),
  phase,
  operationId,
  downloadedBytes,
  totalBytes,
  failure,
  selectionReason,
  allowedActions:
    phase === "setup-required"
      ? ["download"]
      : phase === "waiting-for-lock" || phase === "downloading" || phase === "verifying"
        ? ["cancel"]
        : phase === "failed" || phase === "cancelled"
          ? ["retry"]
          : [],
});

export const validProgress = (
  downloaded: number,
  total: number | null,
  previousDownloaded: number,
  previousTotal: number | null,
  maxBytes: number,
): boolean =>
  Number.isSafeInteger(downloaded) &&
  downloaded >= previousDownloaded &&
  downloaded <= maxBytes &&
  (total === null ||
    (Number.isSafeInteger(total) &&
      total >= downloaded &&
      total <= maxBytes &&
      (previousTotal === null || total >= previousTotal))) &&
  !(previousTotal !== null && total === null);

export const cloneStatus = (status: LocalModelStatus): LocalModelStatus => ({
  ...status,
  model: { ...status.model },
  failure: status.failure === null ? null : { ...status.failure },
  allowedActions: [...status.allowedActions],
});
