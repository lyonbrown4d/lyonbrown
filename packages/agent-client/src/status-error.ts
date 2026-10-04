import { status, type ServiceError } from "@grpc/grpc-js";
import { CommandErrorCode, CommandErrorDetail, UnsupportedCommand } from "@lyonbrown/protocol";

import { clientError, type AgentClientError } from "./errors.js";

export function isRetryableTransportError(error: ServiceError): boolean {
  return error.code === status.UNAVAILABLE || error.code === status.DEADLINE_EXCEEDED;
}

export function mapServiceError(error: ServiceError): AgentClientError {
  const structured = decodeStructuredError(error);
  if (structured !== undefined) return structured;

  switch (error.code) {
    case status.UNAUTHENTICATED:
      return clientError("authentication", "AUTHENTICATION_FAILED", "authentication failed");
    case status.DEADLINE_EXCEEDED:
      return clientError("unavailable", "TIMEOUT", "agent request deadline exceeded");
    case status.UNAVAILABLE:
      return clientError("unavailable", "DISCONNECTED", "agent daemon is unavailable");
    case status.CANCELLED:
      return clientError("cancelled", "CANCELLED", "agent request was cancelled");
    default:
      return clientError("protocol", "UNSUPPORTED", "agent daemon rejected the request", {
        grpcCode: error.code,
      });
  }
}

function decodeStructuredError(error: ServiceError): AgentClientError | undefined {
  const encoded = error.metadata.get("grpc-status-details-bin")[0];
  if (!(encoded instanceof Buffer) || encoded.length === 0) return undefined;
  let detail: ReturnType<typeof CommandErrorDetail.decode>;
  try {
    detail = CommandErrorDetail.decode(encoded);
  } catch {
    return undefined;
  }
  if (detail.code === CommandErrorCode.COMMAND_ERROR_CODE_EPOCH_MISMATCH) {
    if (detail.context?.$case !== "epochMismatch") return undefined;
    return clientError("unavailable", "STALE_SERVER_EPOCH", "agent daemon epoch changed", {
      expectedServerInstanceId: detail.context.epochMismatch.expectedServerInstanceId,
      actualServerInstanceId: detail.context.epochMismatch.actualServerInstanceId,
    });
  }
  return commandError(detail);
}

function commandError(
  detail: ReturnType<typeof CommandErrorDetail.decode>,
): AgentClientError | undefined {
  const plain = (code: Parameters<typeof clientError>[1], message: string): AgentClientError =>
    clientError("command", code, message);
  switch (detail.code) {
    case CommandErrorCode.COMMAND_ERROR_CODE_RUNTIME_STOPPED:
      return plain("RUNTIME_STOPPED", "runtime is stopped");
    case CommandErrorCode.COMMAND_ERROR_CODE_TURN_ALREADY_ACTIVE:
      return plain("TURN_ALREADY_ACTIVE", "turn is already active");
    case CommandErrorCode.COMMAND_ERROR_CODE_NO_ACTIVE_TURN:
      return plain("NO_ACTIVE_TURN", "no turn is active");
    case CommandErrorCode.COMMAND_ERROR_CODE_EMPTY_TEXT:
      return plain("EMPTY_TEXT", "text is empty");
    case CommandErrorCode.COMMAND_ERROR_CODE_STEERING:
      return detail.context?.$case === "steeringReason"
        ? clientError("command", "STEERING", "steering was rejected", {
            reason: detail.context.steeringReason,
          })
        : undefined;
    case CommandErrorCode.COMMAND_ERROR_CODE_NO_PENDING_APPROVAL:
      return plain("NO_PENDING_APPROVAL", "no approval is pending");
    case CommandErrorCode.COMMAND_ERROR_CODE_APPROVAL_REQUEST_MISMATCH:
      return detail.context?.$case === "approvalRequestMismatch"
        ? clientError("command", "APPROVAL_REQUEST_MISMATCH", "approval request does not match", {
            expectedRequestId: detail.context.approvalRequestMismatch.expectedRequestId,
            receivedRequestId: detail.context.approvalRequestMismatch.receivedRequestId,
          })
        : undefined;
    case CommandErrorCode.COMMAND_ERROR_CODE_UNSUPPORTED:
      return unsupportedError(detail);
    case CommandErrorCode.COMMAND_ERROR_CODE_MODEL_SETUP_REQUIRED:
      return plain("MODEL_SETUP_REQUIRED", "model setup is required");
    case CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_UNAVAILABLE:
      return plain("MODEL_DOWNLOAD_UNAVAILABLE", "model download is unavailable");
    case CommandErrorCode.COMMAND_ERROR_CODE_INVALID_MODEL_ID:
      return detail.context?.$case === "invalidModelId"
        ? clientError("command", "INVALID_MODEL_ID", "model id is invalid", {
            requestedModelId: detail.context.invalidModelId.requestedModelId,
            configuredModelId: detail.context.invalidModelId.configuredModelId,
          })
        : undefined;
    case CommandErrorCode.COMMAND_ERROR_CODE_NO_ACTIVE_MODEL_DOWNLOAD:
      return plain("NO_ACTIVE_MODEL_DOWNLOAD", "no model download is active");
    case CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_OPERATION_MISMATCH:
      return detail.context?.$case === "modelDownloadOperationMismatch"
        ? clientError(
            "command",
            "MODEL_DOWNLOAD_OPERATION_MISMATCH",
            "model download operation does not match",
            {
              expectedOperationId:
                detail.context.modelDownloadOperationMismatch.expectedOperationId,
              receivedOperationId:
                detail.context.modelDownloadOperationMismatch.receivedOperationId,
            },
          )
        : undefined;
    case CommandErrorCode.COMMAND_ERROR_CODE_MODEL_DOWNLOAD_CANCELLATION_TOO_LATE:
      return plain(
        "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE",
        "model download cancellation is too late",
      );
    default:
      return undefined;
  }
}

function unsupportedError(
  detail: ReturnType<typeof CommandErrorDetail.decode>,
): AgentClientError | undefined {
  if (detail.context?.$case !== "unsupportedCommand") return undefined;
  const command = unsupportedCommandName(detail.context.unsupportedCommand);
  return command === undefined
    ? undefined
    : clientError("command", "UNSUPPORTED", "command is unsupported", { command });
}

function unsupportedCommandName(value: UnsupportedCommand): string | undefined {
  switch (value) {
    case UnsupportedCommand.UNSUPPORTED_COMMAND_FORCE_STOP:
      return "force-stop";
    case UnsupportedCommand.UNSUPPORTED_COMMAND_RESPOND_APPROVAL:
      return "respond-approval";
    case UnsupportedCommand.UNSUPPORTED_COMMAND_SELECT_SESSION_MODEL:
      return "select-session-model";
    case UnsupportedCommand.UNSUPPORTED_COMMAND_START_MODEL_DOWNLOAD:
      return "start-model-download";
    case UnsupportedCommand.UNSUPPORTED_COMMAND_CANCEL_MODEL_DOWNLOAD:
      return "cancel-model-download";
    default:
      return undefined;
  }
}
