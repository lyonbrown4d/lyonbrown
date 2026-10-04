import { DEFAULT_LOCAL_MODEL } from "../public-model.js";
import type { LocalModelStatus } from "../types.js";
import { DEFAULT_PRESET } from "./catalog.js";

export const statusFor = (
  phase: LocalModelStatus["phase"],
  operationId: string | null = null,
  downloadedBytes = 0,
  totalBytes: number | null = null,
  failure: LocalModelStatus["failure"] = null,
): LocalModelStatus => ({
  model: DEFAULT_LOCAL_MODEL,
  phase,
  operationId,
  downloadedBytes,
  totalBytes,
  failure,
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
): boolean =>
  Number.isSafeInteger(downloaded) &&
  downloaded >= previousDownloaded &&
  downloaded <= DEFAULT_PRESET.maxBytes &&
  (total === null ||
    (Number.isSafeInteger(total) &&
      total >= downloaded &&
      total <= DEFAULT_PRESET.maxBytes &&
      (previousTotal === null || total >= previousTotal))) &&
  !(previousTotal !== null && total === null);

export const cloneStatus = (status: LocalModelStatus): LocalModelStatus => ({
  ...status,
  model: { ...status.model },
  failure: status.failure === null ? null : { ...status.failure },
  allowedActions: [...status.allowedActions],
});
