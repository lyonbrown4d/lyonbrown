import { isProxy } from "node:util/types";

import { PayloadValidationError } from "./errors.js";

export const MAX_EVENT_PAYLOAD_BYTES = 1024 * 1024;
export const MAX_PAYLOAD_DEPTH = 64;
export const MAX_PAYLOAD_NODES = 100_000;

const forbiddenKeys = new Set(["__proto__", "constructor", "prototype"]);
const resolvedSecretKeys = new Set([
  "resolvedValue",
  "resolved_value",
  "resolvedSecret",
  "resolvedSecretValue",
  "secretValue",
]);
const malformedUnicode = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u;

interface TraversalState {
  nodes: number;
  readonly seen: WeakSet<object>;
}

function fail(message: string, cause?: unknown): never {
  throw new PayloadValidationError(message, cause === undefined ? undefined : { cause });
}

function assertSafeString(value: string, role: string): void {
  if (malformedUnicode.test(value)) fail(`${role} contains malformed Unicode`);
}

function visit(value: unknown, depth: number, state: TraversalState): unknown {
  state.nodes += 1;
  if (state.nodes > MAX_PAYLOAD_NODES) {
    fail(`Payload exceeds the ${String(MAX_PAYLOAD_NODES)} node budget`);
  }
  if (depth > MAX_PAYLOAD_DEPTH) {
    fail(`Payload exceeds the depth budget of ${String(MAX_PAYLOAD_DEPTH)}`);
  }

  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") {
    assertSafeString(value, "String");
    return value;
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value)) fail("Payload numbers must be finite");
    return value;
  }
  if (typeof value !== "object") fail(`Payload contains unsupported ${typeof value}`);
  if (isProxy(value)) fail("Payload proxies are not allowed");
  if (state.seen.has(value)) fail("Payload must not contain cycles or repeated object references");
  state.seen.add(value);

  try {
    if (Array.isArray(value)) return visitArray(value, depth, state);
    return visitObject(value, depth, state);
  } catch (error: unknown) {
    if (error instanceof PayloadValidationError) throw error;
    return fail("Payload inspection failed", error);
  } finally {
    state.seen.delete(value);
  }
}

function visitArray(value: readonly unknown[], depth: number, state: TraversalState): unknown[] {
  if (Object.getOwnPropertySymbols(value).length > 0)
    fail("Payload arrays cannot have symbol keys");
  const descriptors = Object.getOwnPropertyDescriptors(value);
  const allowedKeys = new Set(["length"]);
  const result: unknown[] = [];
  for (let index = 0; index < value.length; index += 1) {
    const key = String(index);
    allowedKeys.add(key);
    const descriptor = descriptors[key];
    if (descriptor === undefined || !("value" in descriptor))
      fail("Sparse or accessor arrays are not allowed");
    result.push(visit(descriptor.value, depth + 1, state));
  }
  if (Object.keys(descriptors).some((key) => !allowedKeys.has(key))) {
    fail("Payload arrays cannot have custom properties");
  }
  return result;
}

function visitObject(value: object, depth: number, state: TraversalState): Record<string, unknown> {
  const prototype: unknown = Object.getPrototypeOf(value) as unknown;
  if (prototype !== Object.prototype && prototype !== null) fail("Only plain objects are allowed");
  if (Object.getOwnPropertySymbols(value).length > 0)
    fail("Payload objects cannot have symbol keys");

  const descriptors = Object.getOwnPropertyDescriptors(value);
  const keys = Object.keys(descriptors);
  if (keys.includes("secretRef") && keys.some((key) => resolvedSecretKeys.has(key))) {
    fail("Resolved SecretRef values must never be persisted");
  }

  const result: Record<string, unknown> = Object.create(null) as Record<string, unknown>;
  for (const key of keys) {
    assertSafeString(key, "Object key");
    if (forbiddenKeys.has(key)) fail(`Object key ${JSON.stringify(key)} is forbidden`);
    const descriptor = descriptors[key];
    if (descriptor === undefined || !("value" in descriptor))
      fail("Payload accessors are not allowed");
    const descriptorValue: unknown = descriptor.value as unknown;
    result[key] = visit(descriptorValue, depth + 1, state);
  }
  return result;
}

function clonePayload(value: unknown): unknown {
  return visit(value, 0, { nodes: 0, seen: new WeakSet<object>() });
}

export function encodeEventPayload(value: unknown): string {
  const encoded = JSON.stringify(clonePayload(value));
  if (Buffer.byteLength(encoded, "utf8") > MAX_EVENT_PAYLOAD_BYTES) {
    fail("Encoded payload exceeds the 1 MiB event limit");
  }
  return encoded;
}

export function decodeEventPayload(encoded: string): unknown {
  if (Buffer.byteLength(encoded, "utf8") > MAX_EVENT_PAYLOAD_BYTES) {
    fail("Persisted payload exceeds the 1 MiB event limit");
  }
  try {
    return clonePayload(JSON.parse(encoded) as unknown);
  } catch (error: unknown) {
    if (error instanceof PayloadValidationError) throw error;
    return fail("Persisted payload is not valid JSON", error);
  }
}
