import type { CancelModelDownloadResult, StartModelDownloadResult } from "@lyonbrown/local-model";

import type { RuntimeFailure, RuntimeResult } from "@lyonbrown/agentd/internal/runtime-host";

const OK = Object.freeze({ ok: true }) as RuntimeResult;

export function failedRuntimeResult(error: RuntimeFailure): RuntimeResult {
  return { ok: false, error };
}

export function mapStartDownloadResult(result: StartModelDownloadResult): RuntimeResult {
  switch (result.kind) {
    case "started":
    case "in-progress":
    case "already-ready":
      return OK;
    case "checking":
    case "disposed":
      return { ok: false, error: { code: "MODEL_DOWNLOAD_UNAVAILABLE" } };
  }
}

export function mapCancelDownloadResult(
  result: CancelModelDownloadResult,
  receivedOperationId: string,
): RuntimeResult {
  switch (result.kind) {
    case "cancelled":
      return OK;
    case "no-active-operation":
      return { ok: false, error: { code: "NO_ACTIVE_MODEL_DOWNLOAD" } };
    case "too-late":
      return { ok: false, error: { code: "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE" } };
    case "operation-mismatch":
      return {
        ok: false,
        error: {
          code: "MODEL_DOWNLOAD_OPERATION_MISMATCH",
          expectedOperationId: result.activeOperationId,
          receivedOperationId,
        },
      };
  }
}
