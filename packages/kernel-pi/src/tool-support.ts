import { KernelRuntimeError } from "./errors.js";
import { assertByteLimit, isSecretReference, type SecretRedactor } from "./sanitization.js";
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
  maximumSecretBytes: number,
): Promise<Readonly<Record<string, unknown>>> {
  const resolved = await resolveValue(value, signal, resolve, redactor, maximumSecretBytes);
  signal.throwIfAborted();
  if (!isJsonObject(resolved)) {
    throw new KernelRuntimeError("TOOL_ARGUMENTS_INVALID", "Tool arguments must be an object");
  }
  return resolved;
}

async function resolveValue(
  value: KernelJsonValue,
  signal: AbortSignal,
  resolve: (reference: string, signal: AbortSignal) => Promise<string>,
  redactor: SecretRedactor,
  maximumSecretBytes: number,
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
    assertByteLimit(secret, maximumSecretBytes, "INPUT_BUDGET_EXCEEDED");
    redactor.add(secret);
    return secret;
  }
  if (Array.isArray(value)) {
    const output: KernelJsonValue[] = [];
    const entries = value as readonly KernelJsonValue[];
    for (const entry of entries) {
      output.push(await resolveValue(entry, signal, resolve, redactor, maximumSecretBytes));
    }
    return output;
  }
  if (value !== null && typeof value === "object") {
    const output = Object.create(null) as Record<string, KernelJsonValue>;
    for (const [key, entry] of Object.entries(value)) {
      output[key] = await resolveValue(entry, signal, resolve, redactor, maximumSecretBytes);
    }
    return output;
  }
  return value;
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
