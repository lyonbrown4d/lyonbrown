import { stripVTControlCharacters, types } from "node:util";

import { KernelRuntimeError, type KernelErrorCode } from "./errors.js";
import type { KernelJsonValue, KernelResourceBudget } from "./types.js";

// eslint-disable-next-line no-control-regex -- the sanitizer intentionally removes C0/C1 controls
const CONTROL_CHARACTERS = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/gu;
const BIDI_CONTROLS = /[\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]/gu;
const DANGEROUS_KEYS = new Set(["__proto__", "constructor", "prototype"]);

export interface SanitizationLimits {
  readonly maxDepth: number;
  readonly maxStringBytes: number;
  readonly maxCollectionEntries: number;
  readonly maxNodes: number;
}

interface TraversalState {
  nodes: number;
}

export function limitsFromBudget(budget: KernelResourceBudget): SanitizationLimits {
  return {
    maxDepth: budget.maxValueDepth,
    maxStringBytes: budget.maxValueStringBytes,
    maxCollectionEntries: budget.maxCollectionEntries,
    maxNodes: budget.maxValueNodes,
  };
}

export function sanitizeText(value: string): string {
  return stripVTControlCharacters(toWellFormed(value))
    .replace(CONTROL_CHARACTERS, "")
    .replace(BIDI_CONTROLS, "");
}

export function sanitizeBoundedText(value: string, maximumBytes: number): string {
  assertByteLimit(value, maximumBytes, "INPUT_BUDGET_EXCEEDED");
  return sanitizeText(value);
}

function toWellFormed(value: string): string {
  let output = "";
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        output += value.slice(index, index + 2);
        index += 1;
      } else {
        output += "\uFFFD";
      }
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      output += "\uFFFD";
    } else {
      output += value.charAt(index);
    }
  }
  return output;
}

export function utf8Bytes(value: string): number {
  return Buffer.byteLength(value, "utf8");
}

export function assertByteLimit(value: string, maximum: number, code: KernelErrorCode): void {
  if (utf8Bytes(value) > maximum) {
    throw new KernelRuntimeError(code, `Value exceeds the ${String(maximum)}-byte resource budget`);
  }
}

export function boundedSummary(value: string, maximumBytes = 256): string {
  const sanitized = sanitizeText(value);
  if (utf8Bytes(sanitized) <= maximumBytes) return sanitized;
  let output = "";
  for (const character of sanitized) {
    if (utf8Bytes(output + character + "…") > maximumBytes) break;
    output += character;
  }
  return `${output}…`;
}

export function sanitizeJson(value: unknown, limits: SanitizationLimits): KernelJsonValue {
  return sanitizeValue(value, limits, { nodes: 0 }, 0);
}

function sanitizeValue(
  value: unknown,
  limits: SanitizationLimits,
  state: TraversalState,
  depth: number,
): KernelJsonValue {
  state.nodes += 1;
  if (state.nodes > limits.maxNodes) {
    throw invalidValue("Value exceeds the allowed node budget");
  }
  if (depth > limits.maxDepth) {
    throw invalidValue("Value exceeds the allowed nesting depth");
  }
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") {
    if (utf8Bytes(value) > limits.maxStringBytes) {
      throw invalidValue("Value exceeds the allowed string byte budget");
    }
    return sanitizeText(value);
  }
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value !== "object") throw invalidValue("Value is not JSON-compatible");
  if (types.isProxy(value)) throw invalidValue("Proxy values are not accepted");
  return Array.isArray(value)
    ? sanitizeArray(value, limits, state, depth)
    : sanitizeObject(value, limits, state, depth);
}

function sanitizeArray(
  value: readonly unknown[],
  limits: SanitizationLimits,
  state: TraversalState,
  depth: number,
): KernelJsonValue[] {
  if (value.length > limits.maxCollectionEntries) {
    throw invalidValue("Array exceeds the allowed collection width");
  }
  const descriptors = Object.getOwnPropertyDescriptors(value);
  assertNoAccessors(descriptors);
  const output: KernelJsonValue[] = [];
  for (let index = 0; index < value.length; index += 1) {
    const descriptor = descriptors[String(index)];
    if (!descriptor || !("value" in descriptor)) {
      throw invalidValue("Sparse arrays are not accepted");
    }
    output.push(sanitizeValue(descriptor.value, limits, state, depth + 1));
  }
  const extraKeys = Object.keys(descriptors).filter(
    (key) => key !== "length" && !/^(0|[1-9]\d*)$/u.test(key),
  );
  if (extraKeys.length > 0 || Object.getOwnPropertySymbols(value).length > 0) {
    throw invalidValue("Array contains unsupported properties");
  }
  return output;
}

function sanitizeObject(
  value: object,
  limits: SanitizationLimits,
  state: TraversalState,
  depth: number,
): Readonly<Record<string, KernelJsonValue>> {
  const prototype = Object.getPrototypeOf(value) as object | null;
  if (prototype !== Object.prototype && prototype !== null) {
    throw invalidValue("Only plain JSON objects are accepted");
  }
  if (Object.getOwnPropertySymbols(value).length > 0) {
    throw invalidValue("Symbol keys are not accepted");
  }
  const descriptors = Object.getOwnPropertyDescriptors(value);
  assertNoAccessors(descriptors);
  const entries = Object.entries(descriptors).filter(([, descriptor]) => descriptor.enumerable);
  if (entries.length > limits.maxCollectionEntries) {
    throw invalidValue("Object exceeds the allowed collection width");
  }
  const output = Object.create(null) as Record<string, KernelJsonValue>;
  for (const [rawKey, descriptor] of entries) {
    if (DANGEROUS_KEYS.has(rawKey)) throw invalidValue("Dangerous object key rejected");
    if (!("value" in descriptor)) throw invalidValue("Object accessors are not accepted");
    if (utf8Bytes(rawKey) > limits.maxStringBytes) {
      throw invalidValue("Object key exceeds the allowed string byte budget");
    }
    const key = sanitizeText(rawKey);
    if (Object.hasOwn(output, key)) throw invalidValue("Sanitized object keys collide");
    output[key] = sanitizeValue(descriptor.value, limits, state, depth + 1);
  }
  return output;
}

function assertNoAccessors(descriptors: Readonly<Record<string, PropertyDescriptor>>): void {
  if (Object.values(descriptors).some((descriptor) => "get" in descriptor || "set" in descriptor)) {
    throw invalidValue("Object accessors are not accepted");
  }
}

function invalidValue(message: string): KernelRuntimeError {
  return new KernelRuntimeError("UNTRUSTED_VALUE_INVALID", message);
}

export class SecretRedactor {
  readonly #secrets = new Set<string>();

  add(secret: string): void {
    if (secret.length > 0) this.#secrets.add(secret);
  }

  text(value: string): string {
    let output = sanitizeText(value);
    for (const secret of this.#secrets) output = output.replaceAll(secret, "[REDACTED]");
    return output;
  }

  summary(value: string, maximumBytes = 256): string {
    return boundedSummary(this.text(value), maximumBytes);
  }

  json(value: unknown, limits: SanitizationLimits): KernelJsonValue {
    return redactJson(sanitizeJson(value, limits), (text) => this.text(text));
  }
}

function redactJson(value: KernelJsonValue, redact: (value: string) => string): KernelJsonValue {
  if (typeof value === "string") return redact(value);
  if (Array.isArray(value)) {
    const entries = value as readonly KernelJsonValue[];
    return entries.map((entry) => redactJson(entry, redact));
  }
  if (value !== null && typeof value === "object") {
    const output = Object.create(null) as Record<string, KernelJsonValue>;
    for (const [key, entry] of Object.entries(value)) {
      const redactedKey = redact(key);
      if (Object.hasOwn(output, redactedKey)) {
        throw invalidValue("Redacted object keys collide");
      }
      output[redactedKey] = redactJson(entry, redact);
    }
    return output;
  }
  return value;
}

export function redactSecretReferences(
  value: unknown,
  limits: SanitizationLimits,
): KernelJsonValue {
  return replaceSecretReferences(sanitizeJson(value, limits));
}

function replaceSecretReferences(value: KernelJsonValue): KernelJsonValue {
  if (isSecretReference(value)) return "[REDACTED]";
  if (Array.isArray(value)) {
    const entries = value as readonly KernelJsonValue[];
    return entries.map(replaceSecretReferences);
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, replaceSecretReferences(entry)]),
    );
  }
  return value;
}

export function isSecretReference(
  value: unknown,
): value is { kind: "secretRef"; reference: string } {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  if (types.isProxy(value) || Object.getOwnPropertySymbols(value).length > 0) return false;
  const descriptors = Object.getOwnPropertyDescriptors(value);
  const keys = Object.keys(descriptors);
  if (keys.length !== 2 || !keys.includes("kind") || !keys.includes("reference")) return false;
  const kind = descriptors.kind;
  const reference = descriptors.reference;
  return (
    kind !== undefined &&
    reference !== undefined &&
    "value" in kind &&
    "value" in reference &&
    kind.value === "secretRef" &&
    typeof reference.value === "string"
  );
}
