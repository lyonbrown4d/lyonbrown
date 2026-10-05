import { copyInternalKernelError, kernelError, type KernelRuntimeError } from "./errors.ts";
import { inspectOwnDataProperties } from "./inspection.ts";
import type { SecretRedactor } from "./redaction.ts";
import {
  assertByteLimit,
  isSecretReference,
  jsonStringBudgetBytes,
  utf8Bytes,
  type SanitizationLimits,
} from "./sanitization.ts";
import type { KernelJsonValue, KernelToolDefinition } from "./types.ts";

export function replaceSecretRefsForValidation(
  value: KernelJsonValue,
  maximumKeys: number,
): KernelJsonValue {
  if (isSecretReference(value)) return "[SECRET]";
  if (Array.isArray(value)) {
    const entries = value as readonly KernelJsonValue[];
    return entries.map((entry) => replaceSecretRefsForValidation(entry, maximumKeys));
  }
  if (value !== null && typeof value === "object") {
    const output = Object.create(null) as Record<string, KernelJsonValue>;
    for (const property of inspectOwnDataProperties(value, maximumKeys, inspectionError)) {
      if (property.enumerable) {
        output[property.key] = replaceSecretRefsForValidation(
          property.value as KernelJsonValue,
          maximumKeys,
        );
      }
    }
    return output;
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
    throw kernelError("TOOL_ARGUMENTS_INVALID", "Tool arguments must be an object");
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
      throw kernelError("TOOL_ARGUMENTS_INVALID", "Secret resolution failed");
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
    const entries = inspectOwnDataProperties(
      value,
      limits.maxCollectionEntries,
      inspectionError,
    ).filter((property) => property.enumerable);
    addResolvedBytes(state, limits, 2 + Math.max(0, entries.length - 1));
    for (const property of entries) {
      addResolvedBytes(state, limits, jsonStringBudgetBytes(property.key) + 1);
    }
    const output = Object.create(null) as Record<string, KernelJsonValue>;
    for (const property of entries) {
      output[property.key] = await resolveValue(
        property.value as KernelJsonValue,
        signal,
        resolve,
        redactor,
        limits,
        state,
      );
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
    throw kernelError(
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
  const internal = copyInternalKernelError(error);
  if (internal) return kernelError(internal.code, redactor.summary(internal.message));
  return kernelError("TOOL_DENIED", "Tool authorization unavailable");
}

export function toSafeToolError(error: unknown, redactor: SecretRedactor): KernelRuntimeError {
  const internal = copyInternalKernelError(error);
  return internal
    ? kernelError(internal.code, redactor.summary(internal.message))
    : toExternalToolError();
}

export function toExternalToolError(): KernelRuntimeError {
  return kernelError("MODEL_ERROR", "Tool execution failed");
}

function inspectionError(): Error {
  return kernelError("UNTRUSTED_VALUE_INVALID", "Bounded object inspection failed");
}
