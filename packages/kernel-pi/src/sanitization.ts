import { stripVTControlCharacters, types } from "node:util";

import {
  kernelError,
  type KernelErrorCode,
  type KernelRuntimeError,
} from "@lyonbrown/kernel-pi/internal/errors";
import {
  inspectOwnDataProperties,
  type InspectionFailure,
} from "@lyonbrown/kernel-pi/internal/inspection";
import { isSecretReference } from "@lyonbrown/kernel-pi/internal/secret-reference";
import type { KernelJsonValue, KernelResourceBudget } from "@lyonbrown/kernel-pi/internal/types";

export { isSecretReference } from "@lyonbrown/kernel-pi/internal/secret-reference";

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
    throw kernelError(code, `Value exceeds the ${String(maximum)}-byte resource budget`);
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
  const properties = inspectOwnDataProperties(value, limits.maxCollectionEntries + 1, (failure) =>
    inspectionError("Array", failure),
  );
  const byKey = new Map(properties.map((property) => [property.key, property]));
  const output: KernelJsonValue[] = [];
  for (let index = 0; index < value.length; index += 1) {
    const property = byKey.get(String(index));
    if (!property) {
      throw invalidValue("Sparse arrays are not accepted");
    }
    output.push(sanitizeValue(property.value, limits, state, depth + 1));
  }
  const hasExtraKey = properties.some(
    (property) => property.key !== "length" && !/^(0|[1-9]\d*)$/u.test(property.key),
  );
  if (hasExtraKey) {
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
  const properties = inspectOwnDataProperties(value, limits.maxCollectionEntries, (failure) =>
    inspectionError("Object", failure),
  );
  const entries = properties.filter((property) => property.enumerable);
  addTotalBytes(state, limits, 2 + Math.max(0, entries.length - 1));
  for (const property of entries) {
    addTotalBytes(state, limits, jsonStringBudgetBytes(property.key) + 1);
  }
  const output = Object.create(null) as Record<string, KernelJsonValue>;
  for (const property of entries) {
    const rawKey = property.key;
    if (DANGEROUS_KEYS.has(rawKey)) throw invalidValue("Dangerous object key rejected");
    if (utf8Bytes(rawKey) > limits.maxStringBytes) {
      throw invalidValue("Object key exceeds the allowed string byte budget");
    }
    const key = sanitizeText(rawKey);
    if (DANGEROUS_KEYS.has(key)) throw invalidValue("Dangerous object key rejected");
    if (Object.hasOwn(output, key)) throw invalidValue("Sanitized object keys collide");
    output[key] = sanitizeValue(property.value, limits, state, depth + 1);
  }
  return output;
}

function inspectionError(label: "Array" | "Object", failure: InspectionFailure): Error {
  if (failure === "proxy") return invalidValue("Proxy values are not accepted");
  if (failure === "too_many_keys") {
    return invalidValue(`${label} exceeds the allowed collection width`);
  }
  if (failure === "symbol_key") return invalidValue("Symbol keys are not accepted");
  return invalidValue(`${label} accessors are not accepted`);
}

function invalidValue(message: string): KernelRuntimeError {
  return kernelError("UNTRUSTED_VALUE_INVALID", message);
}

function addTotalBytes(state: TraversalState, limits: SanitizationLimits, bytes: number): void {
  if (bytes > limits.maxTotalBytes - state.totalBytes) {
    throw kernelError(
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
  return replaceSecretReferences(sanitizeJson(value, limits), limits.maxCollectionEntries);
}

function replaceSecretReferences(value: KernelJsonValue, maximumKeys: number): KernelJsonValue {
  if (isSecretReference(value)) return "[REDACTED]";
  if (Array.isArray(value)) {
    const entries = value as readonly KernelJsonValue[];
    return entries.map((entry) => replaceSecretReferences(entry, maximumKeys));
  }
  if (value !== null && typeof value === "object") {
    const output = Object.create(null) as Record<string, KernelJsonValue>;
    const properties = inspectOwnDataProperties(value, maximumKeys, (failure) =>
      inspectionError("Object", failure),
    );
    for (const property of properties) {
      if (property.enumerable) {
        output[property.key] = replaceSecretReferences(
          property.value as KernelJsonValue,
          maximumKeys,
        );
      }
    }
    return output;
  }
  return value;
}
