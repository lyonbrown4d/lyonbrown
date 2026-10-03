import { types } from "node:util";

export type KernelErrorCode =
  | "INPUT_BUDGET_EXCEEDED"
  | "OUTPUT_BUDGET_EXCEEDED"
  | "TURN_BUDGET_EXCEEDED"
  | "STEP_BUDGET_EXCEEDED"
  | "TOOL_RESULT_BUDGET_EXCEEDED"
  | "TOOL_TIMEOUT"
  | "TOOL_OUTCOME_UNKNOWN"
  | "TOOL_DENIED"
  | "TOOL_ARGUMENTS_INVALID"
  | "UNTRUSTED_VALUE_INVALID"
  | "SESSION_BUSY"
  | "EVENT_SINK_UNAVAILABLE"
  | "MODEL_ERROR"
  | "INTERNAL";

const ERROR_CODES = new Set<KernelErrorCode>([
  "INPUT_BUDGET_EXCEEDED",
  "OUTPUT_BUDGET_EXCEEDED",
  "TURN_BUDGET_EXCEEDED",
  "STEP_BUDGET_EXCEEDED",
  "TOOL_RESULT_BUDGET_EXCEEDED",
  "TOOL_TIMEOUT",
  "TOOL_OUTCOME_UNKNOWN",
  "TOOL_DENIED",
  "TOOL_ARGUMENTS_INVALID",
  "UNTRUSTED_VALUE_INVALID",
  "SESSION_BUSY",
  "EVENT_SINK_UNAVAILABLE",
  "MODEL_ERROR",
  "INTERNAL",
]);
const INTERNAL_ERRORS = new WeakSet<KernelRuntimeError>();
const MAX_ERROR_MESSAGE_BYTES = 4_096;

export class KernelRuntimeError extends Error {
  readonly code: KernelErrorCode;

  constructor(code: KernelErrorCode, message: string) {
    super(message);
    this.name = "KernelRuntimeError";
    this.code = code;
  }
}

export function kernelError(code: KernelErrorCode, message: string): KernelRuntimeError {
  const safe =
    ERROR_CODES.has(code) && Buffer.byteLength(message, "utf8") <= MAX_ERROR_MESSAGE_BYTES
      ? new KernelRuntimeError(code, message)
      : new KernelRuntimeError("INTERNAL", "Kernel runtime failed");
  INTERNAL_ERRORS.add(safe);
  return Object.freeze(safe);
}

export function copyInternalKernelError(error: unknown): KernelRuntimeError | undefined {
  if (
    typeof error !== "object" ||
    error === null ||
    types.isProxy(error) ||
    !INTERNAL_ERRORS.has(error as KernelRuntimeError) ||
    !Object.isFrozen(error)
  ) {
    return undefined;
  }
  const code = Object.getOwnPropertyDescriptor(error, "code");
  const message = Object.getOwnPropertyDescriptor(error, "message");
  if (
    !code ||
    !("value" in code) ||
    typeof code.value !== "string" ||
    !ERROR_CODES.has(code.value as KernelErrorCode) ||
    !message ||
    !("value" in message) ||
    typeof message.value !== "string" ||
    Buffer.byteLength(message.value, "utf8") > MAX_ERROR_MESSAGE_BYTES
  ) {
    return undefined;
  }
  return kernelError(code.value as KernelErrorCode, message.value);
}

export function toPublicKernelError(error: unknown): KernelRuntimeError {
  return copyInternalKernelError(error) ?? kernelError("INTERNAL", "Kernel runtime failed");
}
