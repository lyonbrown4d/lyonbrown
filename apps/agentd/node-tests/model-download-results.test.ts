import type { CancelModelDownloadResult, StartModelDownloadResult } from "@lyonbrown/local-model";
import { describe, expect, it } from "vitest";

import {
  mapCancelDownloadResult,
  mapStartDownloadResult,
} from "@lyonbrown/agentd/internal/model-download-results";

describe("model download command result mapping", () => {
  it.each<StartModelDownloadResult>([
    { kind: "started", operationId: "operation-1" },
    { kind: "in-progress", operationId: "operation-1" },
    { kind: "already-ready" },
  ])("accepts the $kind start result", (result) => {
    expect(mapStartDownloadResult(result)).toEqual({ ok: true });
  });

  it.each<StartModelDownloadResult>([{ kind: "checking" }, { kind: "disposed" }])(
    "maps the $kind start result to a stable unavailable failure",
    (result) => {
      expect(mapStartDownloadResult(result)).toEqual({
        ok: false,
        error: { code: "MODEL_DOWNLOAD_UNAVAILABLE" },
      });
    },
  );

  it.each<{
    readonly result: CancelModelDownloadResult;
    readonly expected: ReturnType<typeof mapCancelDownloadResult>;
  }>([
    { result: { kind: "cancelled" }, expected: { ok: true } },
    {
      result: { kind: "no-active-operation" },
      expected: { ok: false, error: { code: "NO_ACTIVE_MODEL_DOWNLOAD" } },
    },
    {
      result: { kind: "too-late" },
      expected: {
        ok: false,
        error: { code: "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE" },
      },
    },
    {
      result: { kind: "operation-mismatch", activeOperationId: "expected-operation" },
      expected: {
        ok: false,
        error: {
          code: "MODEL_DOWNLOAD_OPERATION_MISMATCH",
          expectedOperationId: "expected-operation",
          receivedOperationId: "received-operation",
        },
      },
    },
  ])("maps the $result.kind cancellation result", ({ result, expected }) => {
    expect(mapCancelDownloadResult(result, "received-operation")).toEqual(expected);
  });
});
