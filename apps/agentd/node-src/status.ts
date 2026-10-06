import { Metadata, status, type ServerErrorResponse } from "@grpc/grpc-js";
import type { UnsupportedCommand } from "@lyonbrown/protocol";
import {
  CommandErrorCode,
  CommandErrorDetail,
  EpochMismatchDetail,
  InvalidModelIdDetail,
  ModelDownloadOperationMismatchDetail,
} from "@lyonbrown/protocol";
import type { RuntimeFailure } from "./runtime-host.ts";

const DETAILS_KEY = "grpc-status-details-bin";
const INTERNAL_MESSAGE = "internal daemon error";
const TRUSTED_STATUSES = new WeakSet<object>();

export function trustedStatus(
  code: number,
  message: string,
  metadata?: Metadata,
): ServerErrorResponse {
  const error: ServerErrorResponse = Object.assign(new Error(message), {
    code,
    details: message,
    ...(metadata === undefined ? {} : { metadata }),
  });
  TRUSTED_STATUSES.add(error);
  return error;
}

export function statusAtBoundary(error: unknown): ServerErrorResponse {
  if (typeof error === "object" && error !== null && TRUSTED_STATUSES.has(error)) {
    return error as ServerErrorResponse;
  }
  return trustedStatus(status.INTERNAL, INTERNAL_MESSAGE);
}

export function typedCommandStatus(
  code: number,
  message: string,
  detailCode: CommandErrorCode,
  context?: CommandErrorDetail["context"],
): ServerErrorResponse {
  const metadata = new Metadata();
  const detail = CommandErrorDetail.create({ code: detailCode, context });
  metadata.set(DETAILS_KEY, Buffer.from(CommandErrorDetail.encode(detail).finish()));
  return trustedStatus(code, message, metadata);
}

export function epochMismatchStatus(expected: string, actual: string): ServerErrorResponse {
  return typedCommandStatus(
    status.FAILED_PRECONDITION,
    "server instance changed",
    CommandErrorCode.COMMAND_ERROR_CODE_EPOCH_MISMATCH,
    {
      $case: "epochMismatch",
      epochMismatch: EpochMismatchDetail.create({
        expectedServerInstanceId: expected,
        actualServerInstanceId: actual,
      }),
    },
  );
}

export function unsupportedStatus(command: UnsupportedCommand): ServerErrorResponse {
  return typedCommandStatus(
    status.FAILED_PRECONDITION,
    "command is unsupported",
    CommandErrorCode.COMMAND_ERROR_CODE_UNSUPPORTED,
    { $case: "unsupportedCommand", unsupportedCommand: command },
  );
}

export function runtimeFailureStatus(failure: RuntimeFailure): ServerErrorResponse {
  switch (failure.code) {
    case "RUNTIME_STOPPED":
      return typedCommandStatus(
        status.UNAVAILABLE,
        "runtime stopped",
        CommandErrorCode.COMMAND_ERROR_CODE_RUNTIME_STOPPED,
      );
    case "TURN_ALREADY_ACTIVE":
      return typedCommandStatus(
        status.FAILED_PRECONDITION,
        "turn already active",
        CommandErrorCode.COMMAND_ERROR_CODE_TURN_ALREADY_ACTIVE,
      );
    case "NO_ACTIVE_TURN":
      return typedCommandStatus(
        status.FAILED_PRECONDITION,
        "no active turn",
        CommandErrorCode.COMMAND_ERROR_CODE_NO_ACTIVE_TURN,
      );
    case "EMPTY_TEXT":
      return typedCommandStatus(
        status.INVALID_ARGUMENT,
        "text is empty",
        CommandErrorCode.COMMAND_ERROR_CODE_EMPTY_TEXT,
      );
    case "STEERING":
      return typedCommandStatus(
        status.FAILED_PRECONDITION,
        "steering failed",
        CommandErrorCode.COMMAND_ERROR_CODE_STEERING,
        { $case: "steeringReason", steeringReason: failure.reason },
      );
    case "MODEL_SETUP_REQUIRED":
      return typedCommandStatus(
        status.FAILED_PRECONDITION,
        "model setup required",
        CommandErrorCode.COMMAND_ERROR_CODE_MODEL_SETUP_REQUIRED,
      );
    case "MODEL_DOWNLOAD_UNAVAILABLE":
      return typedCommandStatus(
        status.UNAVAILABLE,
        "model download unavailable",
        CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_UNAVAILABLE,
      );
    case "INVALID_MODEL_ID":
      return typedCommandStatus(
        status.INVALID_ARGUMENT,
        "invalid model id",
        CommandErrorCode.COMMAND_ERROR_CODE_INVALID_MODEL_ID,
        {
          $case: "invalidModelId",
          invalidModelId: InvalidModelIdDetail.create({
            requestedModelId: failure.requestedModelId,
            configuredModelId: failure.configuredModelId,
          }),
        },
      );
    case "NO_ACTIVE_MODEL_DOWNLOAD":
      return typedCommandStatus(
        status.FAILED_PRECONDITION,
        "no active model download",
        CommandErrorCode.COMMAND_ERROR_CODE_NO_ACTIVE_MODEL_DOWNLOAD,
      );
    case "MODEL_DOWNLOAD_OPERATION_MISMATCH":
      return typedCommandStatus(
        status.FAILED_PRECONDITION,
        "model download operation changed",
        CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_OPERATION_MISMATCH,
        {
          $case: "modelDownloadOperationMismatch",
          modelDownloadOperationMismatch: ModelDownloadOperationMismatchDetail.create({
            expectedOperationId: failure.expectedOperationId,
            receivedOperationId: failure.receivedOperationId,
          }),
        },
      );
    case "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE":
      return typedCommandStatus(
        status.FAILED_PRECONDITION,
        "model download cancellation is too late",
        CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_CANCELLATION_TOO_LATE,
      );
    case "RESOURCE_EXHAUSTED":
      return trustedStatus(status.RESOURCE_EXHAUSTED, "runtime queue capacity exceeded");
  }
}
