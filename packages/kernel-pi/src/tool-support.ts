import { types } from "node:util";

import { KernelRuntimeError, safeErrorMessage, type KernelErrorCode } from "./errors.js";
import type { SecretRedactor } from "./redaction.js";
import {
  assertByteLimit,
  isSecretReference,
  jsonStringBudgetBytes,
  utf8Bytes,
  type SanitizationLimits,
} from "./sanitization.js";
import type { KernelJsonValue, KernelToolDefinition } from "./types.js";

export function replaceSecretRefsForValidation(value: KernelJsonValue): KernelJsonValue {
  if (isSecretReference(value)) return "[SECRET]";
  if (Array.isArray(value)) return value.map(replaceSecretRefsForValidation);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, replaceSecretRefsForValidation(entry)]),
    );
  }
  return value;
}

export function isKnownRisk(risk: unknown): risk is KernelToolDefinition["risk"] {
  return risk === "low" || risk === "medium" || risk === "high";
}

export function createDeadlineSignal(
  parent: AbortSignal | undefined,
  timeoutMs: number,
): AbortSignal {
  const timeout = AbortSignal.timeout(timeoutMs);
  return parent ? AbortSignal.any([parent, timeout]) : timeout;
}

export async function raceAbort<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  signal.throwIfAborted();
  return new Promise<T>((resolve, reject) => {
    const onAbort = (): void => {
      reject(new DOMException("Operation aborted", "AbortError"));
    };
    signal.addEventListener("abort", onAbort, { once: true });
    void promise.then(resolve, reject).finally(() => {
      signal.removeEventListener("abort", onAbort);
    });
  });
}

export async function resolveSecrets(
  value: KernelJsonValue,
  signal: AbortSignal,
  resolve: (reference: string, signal: AbortSignal) => Promise<string>,
  redactor: SecretRedactor,
  limits: SanitizationLimits,
): Promise<Readonly<Record<string, unknown>>> {
  const state = { totalBytes: 0 };
  const resolved = await resolveValue(value, signal, resolve, redactor, limits, state);
  signal.throwIfAborted();
  if (!isJsonObject(resolved)) {
    throw new KernelRuntimeError("TOOL_ARGUMENTS_INVALID", "Tool arguments must be an object");
  }
  assertByteLimit(JSON.stringify(resolved), limits.maxTotalBytes, limits.totalBytesErrorCode);
  return resolved;
}

interface ResolutionState {
  totalBytes: number;
}

async function resolveValue(
  value: KernelJsonValue,
  signal: AbortSignal,
  resolve: (reference: string, signal: AbortSignal) => Promise<string>,
  redactor: SecretRedactor,
  limits: SanitizationLimits,
  state: ResolutionState,
): Promise<KernelJsonValue> {
  signal.throwIfAborted();
  if (isSecretReference(value)) {
    let secret: string;
    try {
      secret = await raceAbort(resolve(value.reference, signal), signal);
    } catch (error: unknown) {
      if (signal.aborted) throw error;
      throw new KernelRuntimeError("TOOL_ARGUMENTS_INVALID", "Secret resolution failed");
    }
    signal.throwIfAborted();
    assertByteLimit(secret, limits.maxStringBytes, "INPUT_BUDGET_EXCEEDED");
    addResolvedBytes(state, limits, jsonStringBudgetBytes(secret));
    redactor.add(secret);
    return secret;
  }
  if (Array.isArray(value)) {
    addResolvedBytes(state, limits, 2 + Math.max(0, value.length - 1));
    const output: KernelJsonValue[] = [];
    const entries = value as readonly KernelJsonValue[];
    for (const entry of entries) {
      output.push(await resolveValue(entry, signal, resolve, redactor, limits, state));
    }
    return output;
  }
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value);
    addResolvedBytes(state, limits, 2 + Math.max(0, entries.length - 1));
    for (const [key] of entries) {
      addResolvedBytes(state, limits, jsonStringBudgetBytes(key) + 1);
    }
    const output = Object.create(null) as Record<string, KernelJsonValue>;
    for (const [key, entry] of entries) {
      output[key] = await resolveValue(entry, signal, resolve, redactor, limits, state);
    }
    return output;
  }
  addResolvedBytes(state, limits, primitiveBudgetBytes(value));
  return value;
}

function primitiveBudgetBytes(value: string | number | boolean | null): number {
  if (value === null) return 4;
  if (typeof value === "string") return jsonStringBudgetBytes(value);
  if (typeof value === "boolean") return value ? 4 : 5;
  return utf8Bytes(String(value));
}

function addResolvedBytes(state: ResolutionState, limits: SanitizationLimits, bytes: number): void {
  if (bytes > limits.maxTotalBytes - state.totalBytes) {
    throw new KernelRuntimeError(
      limits.totalBytesErrorCode,
      `Resolved arguments exceed the ${String(limits.maxTotalBytes)}-byte resource budget`,
    );
  }
  state.totalBytes += bytes;
}

export function abortCode(signal: AbortSignal): string {
  return signal.reason instanceof DOMException && signal.reason.name === "TimeoutError"
    ? "TOOL_TIMEOUT"
    : "TOOL_CANCELLED";
}

export function hasAborted(signal: AbortSignal): boolean {
  return signal.aborted;
}

export function isJsonObject(value: unknown): value is Readonly<Record<string, KernelJsonValue>> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function toSafePreExecutionError(
  error: unknown,
  redactor: SecretRedactor,
): KernelRuntimeError {
  const fields = safeKernelErrorFields(error);
  if (fields) return new KernelRuntimeError(fields.code, redactor.summary(fields.message));
  return new KernelRuntimeError("TOOL_DENIED", "Tool authorization unavailable");
}

export function toSafeToolError(error: unknown, redactor: SecretRedactor): KernelRuntimeError {
  const fields = safeKernelErrorFields(error);
  if (fields) return new KernelRuntimeError(fields.code, redactor.summary(fields.message));
  const message = safeErrorMessage(error);
  return new KernelRuntimeError(
    "MODEL_ERROR",
    message === "Unknown runtime error" ? "Tool execution failed" : redactor.summary(message),
  );
}

const KERNEL_ERROR_CODES = new Set<KernelErrorCode>([
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
]);

function safeKernelErrorFields(
  error: unknown,
): { readonly code: KernelErrorCode; readonly message: string } | undefined {
  if (
    typeof error !== "object" ||
    error === null ||
    types.isProxy(error) ||
    !(error instanceof KernelRuntimeError)
  ) {
    return undefined;
  }
  const code = Object.getOwnPropertyDescriptor(error, "code");
  const message = Object.getOwnPropertyDescriptor(error, "message");
  if (
    !code ||
    !("value" in code) ||
    typeof code.value !== "string" ||
    !KERNEL_ERROR_CODES.has(code.value as KernelErrorCode) ||
    !message ||
    !("value" in message) ||
    typeof message.value !== "string" ||
    utf8Bytes(message.value) > 4_096
  ) {
    return undefined;
  }
  return { code: code.value as KernelErrorCode, message: message.value };
}
