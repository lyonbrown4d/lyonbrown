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
  | "MODEL_ERROR";

export class KernelRuntimeError extends Error {
  readonly code: KernelErrorCode;

  constructor(code: KernelErrorCode, message: string) {
    super(message);
    this.name = "KernelRuntimeError";
    this.code = code;
  }
}

export function asSafeError(error: unknown): Error {
  return error instanceof Error ? error : new Error("Unknown runtime error");
}

export function safeErrorMessage(error: unknown): string {
  if (typeof error !== "object" || error === null || types.isProxy(error)) {
    return "Unknown runtime error";
  }
  const descriptor = Object.getOwnPropertyDescriptor(error, "message");
  if (!descriptor || !("value" in descriptor) || typeof descriptor.value !== "string") {
    return "Unknown runtime error";
  }
  return Buffer.byteLength(descriptor.value, "utf8") <= 4_096
    ? descriptor.value
    : "Runtime error message exceeded resource budget";
}
import { types } from "node:util";
