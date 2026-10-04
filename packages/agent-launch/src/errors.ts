export type AgentLaunchErrorKind =
  | "validation"
  | "authentication"
  | "unavailable"
  | "protocol"
  | "cancelled";

export type AgentLaunchErrorCode =
  | "INVALID_CONFIGURATION"
  | "CONTROL_PLANE_FAILED"
  | "AUTO_START_DISABLED"
  | "STARTUP_TIMEOUT"
  | "READINESS_TIMEOUT"
  | "SPAWN_FAILED"
  | "DAEMON_EXITED"
  | "AUTHENTICATION_FAILED"
  | "SERVER_INSTANCE_MISMATCH"
  | "SCOPE_MISMATCH"
  | "CLIENT_FAILED"
  | "EPOCH_UNSTABLE"
  | "CANCELLED";

export type AgentLaunchErrorDetails = Readonly<Record<string, string | number | boolean>>;

const AGENT_LAUNCH_ERRORS = new WeakSet<object>();

export class AgentLaunchError extends Error {
  public override readonly name = "AgentLaunchError";

  public constructor(
    public readonly kind: AgentLaunchErrorKind,
    public readonly code: AgentLaunchErrorCode,
    message: string,
    public readonly details?: AgentLaunchErrorDetails,
  ) {
    super(message);
    AGENT_LAUNCH_ERRORS.add(this);
    Object.freeze(this);
  }
}

export function isAgentLaunchError(error: unknown): error is AgentLaunchError {
  if ((typeof error !== "object" || error === null) && typeof error !== "function") return false;
  return AGENT_LAUNCH_ERRORS.has(error);
}

export function launchError(
  kind: AgentLaunchErrorKind,
  code: AgentLaunchErrorCode,
  message: string,
  details?: AgentLaunchErrorDetails,
): AgentLaunchError {
  return new AgentLaunchError(kind, code, message, details);
}
