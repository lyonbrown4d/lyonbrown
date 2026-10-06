import { Metadata, status, type ServiceError } from "@grpc/grpc-js";
import { CommandErrorCode, CommandErrorDetail, UnsupportedCommand } from "@lyonbrown/protocol";
import { describe, expect, it } from "vitest";

import {
  isRetryableTransportError,
  mapServiceError,
} from "@lyonbrown/agent-client/internal/status-error";

function serviceError(code: number, detail?: ReturnType<typeof CommandErrorDetail.fromPartial>) {
  const metadata = new Metadata();
  if (detail !== undefined) {
    metadata.set(
      "grpc-status-details-bin",
      Buffer.from(CommandErrorDetail.encode(detail).finish()),
    );
  }
  return Object.assign(new Error("private transport detail"), {
    code,
    details: "private transport detail",
    metadata,
  }) as ServiceError;
}

describe("grpc status mapping", () => {
  it.each([
    [status.UNAUTHENTICATED, "authentication", "AUTHENTICATION_FAILED"],
    [status.DEADLINE_EXCEEDED, "unavailable", "TIMEOUT"],
    [status.UNAVAILABLE, "unavailable", "DISCONNECTED"],
    [status.CANCELLED, "cancelled", "CANCELLED"],
    [status.INVALID_ARGUMENT, "protocol", "UNSUPPORTED"],
  ])("maps coarse status %s", (wire, kind, code) => {
    expect(mapServiceError(serviceError(wire))).toMatchObject({ kind, code });
  });

  it("only retries transport availability failures", () => {
    expect(isRetryableTransportError(serviceError(status.UNAVAILABLE))).toBe(true);
    expect(isRetryableTransportError(serviceError(status.DEADLINE_EXCEEDED))).toBe(true);
    expect(isRetryableTransportError(serviceError(status.INVALID_ARGUMENT))).toBe(false);
  });

  it.each([
    [CommandErrorCode.COMMAND_ERROR_CODE_RUNTIME_STOPPED, "RUNTIME_STOPPED"],
    [CommandErrorCode.COMMAND_ERROR_CODE_TURN_ALREADY_ACTIVE, "TURN_ALREADY_ACTIVE"],
    [CommandErrorCode.COMMAND_ERROR_CODE_NO_ACTIVE_TURN, "NO_ACTIVE_TURN"],
    [CommandErrorCode.COMMAND_ERROR_CODE_EMPTY_TEXT, "EMPTY_TEXT"],
    [CommandErrorCode.COMMAND_ERROR_CODE_NO_PENDING_APPROVAL, "NO_PENDING_APPROVAL"],
    [CommandErrorCode.COMMAND_ERROR_CODE_MODEL_SETUP_REQUIRED, "MODEL_SETUP_REQUIRED"],
    [CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_UNAVAILABLE, "MODEL_DOWNLOAD_UNAVAILABLE"],
    [CommandErrorCode.COMMAND_ERROR_CODE_NO_ACTIVE_MODEL_DOWNLOAD, "NO_ACTIVE_MODEL_DOWNLOAD"],
    [
      CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_CANCELLATION_TOO_LATE,
      "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE",
    ],
  ])("maps command detail %s", (wire, code) => {
    const detail = CommandErrorDetail.fromPartial({ code: wire });
    expect(mapServiceError(serviceError(status.FAILED_PRECONDITION, detail))).toMatchObject({
      kind: "command",
      code,
    });
  });

  it.each([
    [
      CommandErrorDetail.fromPartial({
        code: CommandErrorCode.COMMAND_ERROR_CODE_STEERING,
        context: { $case: "steeringReason", steeringReason: "too late" },
      }),
      "STEERING",
    ],
    [
      CommandErrorDetail.fromPartial({
        code: CommandErrorCode.COMMAND_ERROR_CODE_APPROVAL_REQUEST_MISMATCH,
        context: {
          $case: "approvalRequestMismatch",
          approvalRequestMismatch: { expectedRequestId: "a", receivedRequestId: "b" },
        },
      }),
      "APPROVAL_REQUEST_MISMATCH",
    ],
    [
      CommandErrorDetail.fromPartial({
        code: CommandErrorCode.COMMAND_ERROR_CODE_INVALID_MODEL_ID,
        context: {
          $case: "invalidModelId",
          invalidModelId: { requestedModelId: "a", configuredModelId: "b" },
        },
      }),
      "INVALID_MODEL_ID",
    ],
    [
      CommandErrorDetail.fromPartial({
        code: CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_OPERATION_MISMATCH,
        context: {
          $case: "modelDownloadOperationMismatch",
          modelDownloadOperationMismatch: { expectedOperationId: "a", receivedOperationId: "b" },
        },
      }),
      "MODEL_DOWNLOAD_OPERATION_MISMATCH",
    ],
    [
      CommandErrorDetail.fromPartial({
        code: CommandErrorCode.COMMAND_ERROR_CODE_EPOCH_MISMATCH,
        context: {
          $case: "epochMismatch",
          epochMismatch: { expectedServerInstanceId: "a", actualServerInstanceId: "b" },
        },
      }),
      "STALE_SERVER_EPOCH",
    ],
  ])("maps contextual command error %#", (detail, code) => {
    expect(mapServiceError(serviceError(status.FAILED_PRECONDITION, detail))).toMatchObject({
      code,
    });
  });

  it.each([
    UnsupportedCommand.UNSUPPORTED_COMMAND_FORCE_STOP,
    UnsupportedCommand.UNSUPPORTED_COMMAND_RESPOND_APPROVAL,
    UnsupportedCommand.UNSUPPORTED_COMMAND_SELECT_SESSION_MODEL,
    UnsupportedCommand.UNSUPPORTED_COMMAND_START_MODEL_DOWNLOAD,
    UnsupportedCommand.UNSUPPORTED_COMMAND_CANCEL_MODEL_DOWNLOAD,
  ])("maps unsupported command %s", (unsupportedCommand) => {
    const detail = CommandErrorDetail.fromPartial({
      code: CommandErrorCode.COMMAND_ERROR_CODE_UNSUPPORTED,
      context: { $case: "unsupportedCommand", unsupportedCommand },
    });
    expect(mapServiceError(serviceError(status.UNIMPLEMENTED, detail))).toMatchObject({
      kind: "command",
      code: "UNSUPPORTED",
    });
  });

  it("falls back when binary status details are malformed or incomplete", () => {
    const malformed = serviceError(status.INTERNAL);
    malformed.metadata.set("grpc-status-details-bin", Buffer.from([255]));
    expect(mapServiceError(malformed)).toMatchObject({ kind: "protocol", code: "UNSUPPORTED" });

    const incomplete = CommandErrorDetail.fromPartial({
      code: CommandErrorCode.COMMAND_ERROR_CODE_STEERING,
    });
    expect(mapServiceError(serviceError(status.INTERNAL, incomplete))).toMatchObject({
      kind: "protocol",
      code: "UNSUPPORTED",
    });
  });
});
