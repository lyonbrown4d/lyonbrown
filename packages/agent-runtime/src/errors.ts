export type AgentRuntimeErrorCode =
  | "EVENT_SINK_UNAVAILABLE"
  | "EVENT_STREAM_CORRUPT"
  | "KERNEL_FAILURE"
  | "RUNTIME_DISPOSED"
  | "SESSION_BUSY"
  | "TERMINAL_SESSION";

export class AgentRuntimeError extends Error {
  readonly code: AgentRuntimeErrorCode;

  constructor(code: AgentRuntimeErrorCode, message: string) {
    super(message);
    this.name = "AgentRuntimeError";
    this.code = code;
  }
}

export function runtimeError(code: AgentRuntimeErrorCode, message: string): AgentRuntimeError {
  return Object.freeze(new AgentRuntimeError(code, message));
}
