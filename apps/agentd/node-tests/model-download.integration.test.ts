import { status } from "@grpc/grpc-js";
import { CancelModelDownloadRequest, StartModelDownloadRequest } from "@lyonbrown/protocol";
import { CommandErrorCode } from "@lyonbrown/protocol";
import { afterAll, describe, expect, it } from "vitest";

import {
  authorization,
  detailFrom,
  invokeUnary,
  requestMetadata,
  startTestServer,
  type TestServerState,
} from "./server-test-support.ts";

describe("model download RPCs", () => {
  const states: TestServerState[] = [];
  afterAll(async () => Promise.all(states.map(async (state) => state.close())));

  it("routes start and cancel through replay-protected RuntimeHost commands", async () => {
    const state = await startTestServer();
    states.push(state);
    const metadata = authorization(state.token);
    await invokeUnary((callback) =>
      state.client.startModelDownload(
        StartModelDownloadRequest.create({
          metadata: requestMetadata("download"),
          modelId: "qwen3-1.7b-q4_k_m",
        }),
        metadata,
        callback,
      ),
    );
    await invokeUnary((callback) =>
      state.client.cancelModelDownload(
        CancelModelDownloadRequest.create({
          metadata: requestMetadata("cancel-download"),
          operationId: "operation-1",
        }),
        metadata,
        callback,
      ),
    );
    expect(state.host.execute.mock.calls.map(([command]) => command)).toEqual([
      { kind: "start-model-download", modelId: "qwen3-1.7b-q4_k_m" },
      { kind: "cancel-model-download", operationId: "operation-1" },
    ]);
  });

  it("returns typed model download failures with mismatch context", async () => {
    const state = await startTestServer();
    states.push(state);
    const metadata = authorization(state.token);
    const cases = [
      {
        command: { kind: "start-model-download", modelId: "wrong" } as const,
        error: {
          code: "INVALID_MODEL_ID",
          requestedModelId: "wrong",
          configuredModelId: "configured",
        } as const,
        invoke: () =>
          invokeUnary((callback) =>
            state.client.startModelDownload(
              StartModelDownloadRequest.create({
                metadata: requestMetadata("invalid-model"),
                modelId: "wrong",
              }),
              metadata,
              callback,
            ),
          ),
        grpc: status.INVALID_ARGUMENT,
        detail: CommandErrorCode.COMMAND_ERROR_CODE_INVALID_MODEL_ID,
      },
      {
        command: { kind: "start-model-download", modelId: "configured" } as const,
        error: { code: "MODEL_DOWNLOAD_UNAVAILABLE" } as const,
        invoke: () =>
          invokeUnary((callback) =>
            state.client.startModelDownload(
              StartModelDownloadRequest.create({
                metadata: requestMetadata("unavailable"),
                modelId: "configured",
              }),
              metadata,
              callback,
            ),
          ),
        grpc: status.UNAVAILABLE,
        detail: CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_UNAVAILABLE,
      },
      {
        command: { kind: "cancel-model-download", operationId: "received" } as const,
        error: {
          code: "MODEL_DOWNLOAD_OPERATION_MISMATCH",
          expectedOperationId: "expected",
          receivedOperationId: "received",
        } as const,
        invoke: () =>
          invokeUnary((callback) =>
            state.client.cancelModelDownload(
              CancelModelDownloadRequest.create({
                metadata: requestMetadata("mismatch"),
                operationId: "received",
              }),
              metadata,
              callback,
            ),
          ),
        grpc: status.FAILED_PRECONDITION,
        detail: CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_OPERATION_MISMATCH,
      },
      {
        command: { kind: "cancel-model-download", operationId: "late" } as const,
        error: { code: "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE" } as const,
        invoke: () =>
          invokeUnary((callback) =>
            state.client.cancelModelDownload(
              CancelModelDownloadRequest.create({
                metadata: requestMetadata("too-late"),
                operationId: "late",
              }),
              metadata,
              callback,
            ),
          ),
        grpc: status.FAILED_PRECONDITION,
        detail: CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_CANCELLATION_TOO_LATE,
      },
    ];
    for (const value of cases) {
      state.host.execute.mockResolvedValueOnce({ ok: false, error: value.error });
      const caught = await value.invoke().catch((error: unknown) => error);
      expect(caught).toMatchObject({ code: value.grpc });
      expect(detailFrom(caught).code).toBe(value.detail);
      if (value.detail === CommandErrorCode.COMMAND_ERROR_CODE_INVALID_MODEL_ID) {
        expect(detailFrom(caught).context).toMatchObject({
          $case: "invalidModelId",
          invalidModelId: { requestedModelId: "wrong", configuredModelId: "configured" },
        });
      }
      if (value.detail === CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_OPERATION_MISMATCH) {
        expect(detailFrom(caught).context).toMatchObject({
          $case: "modelDownloadOperationMismatch",
          modelDownloadOperationMismatch: {
            expectedOperationId: "expected",
            receivedOperationId: "received",
          },
        });
      }
      expect(state.host.execute).toHaveBeenLastCalledWith(value.command);
    }
  });

  it("replays both successful and failed model commands without re-execution", async () => {
    const state = await startTestServer();
    states.push(state);
    const metadata = authorization(state.token);
    const success = StartModelDownloadRequest.create({
      metadata: requestMetadata("replay-success"),
      modelId: "configured",
    });
    const invokeStart = (request: StartModelDownloadRequest) =>
      invokeUnary((callback) => state.client.startModelDownload(request, metadata, callback));
    await invokeStart(success);
    await invokeStart(success);
    expect(state.host.execute).toHaveBeenCalledTimes(1);

    state.host.execute.mockResolvedValueOnce({
      ok: false,
      error: { code: "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE" },
    });
    const failed = CancelModelDownloadRequest.create({
      metadata: requestMetadata("replay-failure"),
      operationId: "late",
    });
    const invokeCancel = () =>
      invokeUnary((callback) => state.client.cancelModelDownload(failed, metadata, callback));
    const first = await invokeCancel().catch((error: unknown) => error);
    const second = await invokeCancel().catch((error: unknown) => error);
    expect(detailFrom(first).code).toBe(
      CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_CANCELLATION_TOO_LATE,
    );
    expect(detailFrom(second).code).toBe(
      CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_CANCELLATION_TOO_LATE,
    );
    expect(state.host.execute).toHaveBeenCalledTimes(2);
  });
});
