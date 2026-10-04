import { types as nodeTypes } from "node:util";

import { domainError } from "./errors.js";
import { isWellFormedString } from "./unicode.js";

export type RuntimeJsonPrimitive = null | boolean | number | string;
export type RuntimeJsonValue =
  | RuntimeJsonPrimitive
  | readonly RuntimeJsonValue[]
  | { readonly [key: string]: RuntimeJsonValue };

const MAX_DEPTH = 32;
const MAX_NODES = 8_192;
const MAX_ENTRIES = 2_048;
export const RUNTIME_JSON_MAX_STRING_BYTES = 64 * 1_024;
export const RUNTIME_JSON_MAX_TOTAL_STRING_BYTES = 256 * 1_024;
const DANGEROUS_KEYS = new Set(["__proto__", "prototype", "constructor"]);

interface CopyState {
  readonly ancestors: WeakSet<object>;
  nodes: number;
  stringBytes: number;
}

function invalid(reason: string): never {
  throw domainError("invalid_argument", "Runtime event must contain bounded JSON data", {
    field: "event",
    reason,
  });
}

export function copyRuntimeJson(value: unknown): RuntimeJsonValue {
  return copyValue(value, { ancestors: new WeakSet(), nodes: 0, stringBytes: 0 }, 0);
}

function copyValue(value: unknown, state: CopyState, depth: number): RuntimeJsonValue {
  if (depth > MAX_DEPTH) return invalid("json_depth_exceeded");
  if (state.nodes >= MAX_NODES) return invalid("json_nodes_exceeded");
  state.nodes += 1;

  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") {
    if (!isWellFormedString(value)) return invalid("ill_formed_unicode");
    chargeStringBytes(value, state);
    return value;
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value))) {
      return invalid("invalid_number");
    }
    return value;
  }
  if (typeof value !== "object" || nodeTypes.isProxy(value)) {
    return invalid("unsupported_json_value");
  }
  if (state.ancestors.has(value)) return invalid("cyclic_json_value");
  state.ancestors.add(value);
  try {
    return Array.isArray(value) ? copyArray(value, state, depth) : copyObject(value, state, depth);
  } finally {
    state.ancestors.delete(value);
  }
}

function copyArray(value: readonly unknown[], state: CopyState, depth: number): RuntimeJsonValue {
  if (Reflect.getPrototypeOf(value) !== Array.prototype || value.length > MAX_ENTRIES) {
    return invalid("invalid_json_array");
  }
  const result: RuntimeJsonValue[] = [];
  for (let index = 0; index < value.length; index += 1) {
    const descriptor = Reflect.getOwnPropertyDescriptor(value, String(index));
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      return invalid("sparse_or_accessor_array");
    }
    result.push(copyValue(descriptor.value as unknown, state, depth + 1));
  }
  if (Reflect.ownKeys(value).length !== value.length + 1) {
    return invalid("array_extra_property");
  }
  return Object.freeze(result);
}

function copyObject(value: object, state: CopyState, depth: number): RuntimeJsonValue {
  const prototype = Reflect.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    return invalid("unsupported_json_object");
  }
  const keys = Reflect.ownKeys(value);
  if (keys.length > MAX_ENTRIES || keys.some((key) => typeof key === "symbol")) {
    return invalid("invalid_json_object_keys");
  }
  const result: Record<string, RuntimeJsonValue> = Object.create(null) as Record<
    string,
    RuntimeJsonValue
  >;
  for (const key of keys as string[]) {
    if (DANGEROUS_KEYS.has(key) || !isWellFormedString(key)) return invalid("dangerous_json_key");
    chargeStringBytes(key, state);
    const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      return invalid("accessor_json_property");
    }
    result[key] = copyValue(descriptor.value as unknown, state, depth + 1);
  }
  return Object.freeze(result);
}

function chargeStringBytes(value: string, state: CopyState): void {
  const bytes = Buffer.byteLength(value, "utf8");
  if (bytes > RUNTIME_JSON_MAX_STRING_BYTES) {
    throw domainError("resource_exhausted", "Runtime JSON string bytes exceed the limit", {
      resource: "runtime_json_string_bytes",
      limitBytes: RUNTIME_JSON_MAX_STRING_BYTES,
    });
  }
  if (bytes > RUNTIME_JSON_MAX_TOTAL_STRING_BYTES - state.stringBytes) {
    throw domainError("resource_exhausted", "Runtime JSON total string bytes exceed the limit", {
      resource: "runtime_json_total_string_bytes",
      limitBytes: RUNTIME_JSON_MAX_TOTAL_STRING_BYTES,
    });
  }
  state.stringBytes += bytes;
}
