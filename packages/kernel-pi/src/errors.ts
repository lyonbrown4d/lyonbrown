export type KernelErrorCode =
  | "INPUT_BUDGET_EXCEEDED"
  | "OUTPUT_BUDGET_EXCEEDED"
  | "TURN_BUDGET_EXCEEDED"
  | "STEP_BUDGET_EXCEEDED"
  | "TOOL_RESULT_BUDGET_EXCEEDED"
  | "TOOL_TIMEOUT"
  | "TOOL_DENIED"
  | "TOOL_ARGUMENTS_INVALID"
  | "UNTRUSTED_VALUE_INVALID"
  | "SESSION_BUSY"
  | "MODEL_ERROR";

export class KernelRuntimeError extends Error {
  readonly code: KernelErrorCode;

  constructor(code: KernelErrorCode, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "KernelRuntimeError";
    this.code = code;
  }
}

export function asSafeError(error: unknown): Error {
  return error instanceof Error ? error : new Error("Unknown runtime error");
}
