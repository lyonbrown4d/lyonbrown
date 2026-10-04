export type AgentClientErrorKind =
  | "validation"
  | "authentication"
  | "unavailable"
  | "protocol"
  | "command"
  | "cancelled";

export type AgentClientErrorCode =
  | "INVALID_ARGUMENT"
  | "AUTHENTICATION_FAILED"
  | "DISCONNECTED"
  | "TIMEOUT"
  | "STALE_SERVER_EPOCH"
  | "CANCELLED"
  | "NOT_HANDSHAKEN"
  | "INCOMPATIBLE_VERSION"
  | "SCOPE_MISMATCH"
  | "SERVER_INSTANCE_MISMATCH"
  | "INVALID_RESPONSE"
  | "INVALID_RECEIPT"
  | "INVALID_SNAPSHOT"
  | "REVISION_MISMATCH"
  | "RUNTIME_STOPPED"
  | "TURN_ALREADY_ACTIVE"
  | "NO_ACTIVE_TURN"
  | "EMPTY_TEXT"
  | "STEERING"
  | "NO_PENDING_APPROVAL"
  | "APPROVAL_REQUEST_MISMATCH"
  | "UNSUPPORTED"
  | "MODEL_SETUP_REQUIRED"
  | "MODEL_DOWNLOAD_UNAVAILABLE"
  | "INVALID_MODEL_ID"
  | "NO_ACTIVE_MODEL_DOWNLOAD"
  | "MODEL_DOWNLOAD_OPERATION_MISMATCH"
  | "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE";

export type AgentClientErrorDetails = Readonly<Record<string, string | number | bigint | boolean>>;

export class AgentClientError extends Error {
  public override readonly name = "AgentClientError";

  public constructor(
    public readonly kind: AgentClientErrorKind,
    public readonly code: AgentClientErrorCode,
    message: string,
    public readonly details?: AgentClientErrorDetails,
  ) {
    super(message);
    Object.freeze(this);
  }
}

export function clientError(
  kind: AgentClientErrorKind,
  code: AgentClientErrorCode,
  message: string,
  details?: AgentClientErrorDetails,
): AgentClientError {
  return new AgentClientError(kind, code, message, details);
}
