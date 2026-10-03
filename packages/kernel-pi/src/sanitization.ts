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
  readonly maxTotalBytes: number;
  readonly totalBytesErrorCode: KernelErrorCode;
}

interface TraversalState {
  nodes: number;
  totalBytes: number;
}

export function limitsFromBudget(
  budget: KernelResourceBudget,
  maxTotalBytes = budget.maxInputBytes,
  totalBytesErrorCode: KernelErrorCode = "INPUT_BUDGET_EXCEEDED",
): SanitizationLimits {
  return {
    maxDepth: budget.maxValueDepth,
    maxStringBytes: budget.maxValueStringBytes,
    maxCollectionEntries: budget.maxCollectionEntries,
    maxNodes: budget.maxValueNodes,
    maxTotalBytes,
    totalBytesErrorCode,
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
  return sanitizeValue(value, limits, { nodes: 0, totalBytes: 0 }, 0);
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
  if (value === null) {
    addTotalBytes(state, limits, 4);
    return value;
  }
  if (typeof value === "boolean") {
    addTotalBytes(state, limits, value ? 4 : 5);
    return value;
  }
  if (typeof value === "string") {
    const bytes = utf8Bytes(value);
    addTotalBytes(state, limits, jsonStringBudgetBytes(value));
    if (bytes > limits.maxStringBytes) {
      throw invalidValue("Value exceeds the allowed string byte budget");
    }
    return sanitizeText(value);
  }
  if (typeof value === "number" && Number.isFinite(value)) {
    addTotalBytes(state, limits, utf8Bytes(String(value)));
    return value;
  }
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
  addTotalBytes(state, limits, 2 + Math.max(0, value.length - 1));
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
  addTotalBytes(state, limits, 2 + Math.max(0, entries.length - 1));
  for (const [rawKey] of entries) {
    addTotalBytes(state, limits, jsonStringBudgetBytes(rawKey) + 1);
  }
  const output = Object.create(null) as Record<string, KernelJsonValue>;
  for (const [rawKey, descriptor] of entries) {
    if (DANGEROUS_KEYS.has(rawKey)) throw invalidValue("Dangerous object key rejected");
    if (!("value" in descriptor)) throw invalidValue("Object accessors are not accepted");
    if (utf8Bytes(rawKey) > limits.maxStringBytes) {
      throw invalidValue("Object key exceeds the allowed string byte budget");
    }
    const key = sanitizeText(rawKey);
    if (DANGEROUS_KEYS.has(key)) throw invalidValue("Dangerous object key rejected");
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

function addTotalBytes(state: TraversalState, limits: SanitizationLimits, bytes: number): void {
  if (bytes > limits.maxTotalBytes - state.totalBytes) {
    throw new KernelRuntimeError(
      limits.totalBytesErrorCode,
      `Value exceeds the ${String(limits.maxTotalBytes)}-byte aggregate resource budget`,
    );
  }
  state.totalBytes += bytes;
}

export function jsonStringBudgetBytes(value: string): number {
  let bytes = utf8Bytes(value) + 2;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (
      code === 0x22 ||
      code === 0x5c ||
      code === 0x08 ||
      code === 0x09 ||
      code === 0x0a ||
      code === 0x0c ||
      code === 0x0d
    ) {
      bytes += 1;
    } else if (code <= 0x1f) {
      bytes += 5;
    }
  }
  return bytes;
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
