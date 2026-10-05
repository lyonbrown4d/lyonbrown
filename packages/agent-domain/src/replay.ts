import { createHash } from "node:crypto";
import { types as nodeTypes } from "node:util";

import { domainError } from "./errors.ts";
import { assertIdentifier, type ClientInstanceId, type CommandId } from "./identifiers.ts";
import { isWellFormedString } from "./unicode.ts";

declare const replayKeyBrand: unique symbol;
declare const commandFingerprintBrand: unique symbol;

export type ReplayKey = string & { readonly [replayKeyBrand]: true };
export type CommandFingerprint = string & { readonly [commandFingerprintBrand]: true };

/** Hard limits for untrusted values entering the replay-fingerprint boundary. */
export const REPLAY_FINGERPRINT_MAX_DEPTH = 64;
export const REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH = 4_096;
export const REPLAY_FINGERPRINT_MAX_BYTE_ARRAY_BYTES = 256 * 1_024;
export const REPLAY_FINGERPRINT_MAX_NODES = 16_384;
export const REPLAY_FINGERPRINT_MAX_ENCODED_BYTES = 2 * 1_024 * 1_024;

const DANGEROUS_KEYS = new Set(["__proto__", "prototype", "constructor"]);
const BUFFER_PROTOTYPE = Reflect.getPrototypeOf(Buffer.alloc(0));

interface EncodingState {
  readonly ancestors: WeakSet<object>;
  readonly onChunk: (chunk: string) => void;
  encodedBytes: number;
  nodes: number;
}

function reject(reason: string, extra: { key?: string } = {}): never {
  throw domainError("invalid_argument", `Value cannot be canonically encoded: ${reason}`, {
    field: "value",
    reason,
    ...extra,
  });
}

function rejectCountBudget(resource: string, limit: number): never {
  throw domainError("resource_exhausted", `Replay fingerprint ${resource} limit exceeded`, {
    resource,
    limit: BigInt(limit),
  });
}

function rejectByteBudget(resource: string, limitBytes: number): never {
  throw domainError("resource_exhausted", `Replay fingerprint ${resource} limit exceeded`, {
    resource,
    limitBytes,
  });
}

function ensureEncodedCapacity(state: EncodingState, additionalBytes: number): void {
  if (additionalBytes > REPLAY_FINGERPRINT_MAX_ENCODED_BYTES - state.encodedBytes) {
    rejectByteBudget("replay_fingerprint_encoded_bytes", REPLAY_FINGERPRINT_MAX_ENCODED_BYTES);
  }
}

function appendKnownBytes(state: EncodingState, chunk: string, bytes: number): void {
  ensureEncodedCapacity(state, bytes);
  state.encodedBytes += bytes;
  state.onChunk(chunk);
}

function appendAscii(state: EncodingState, chunk: string): void {
  appendKnownBytes(state, chunk, chunk.length);
}

function appendFrame(state: EncodingState, tag: string, value: string): void {
  const valueBytes = Buffer.byteLength(value, "utf8");
  const header = `${tag}${String(valueBytes)}:`;
  ensureEncodedCapacity(state, header.length + valueBytes);
  appendKnownBytes(state, header, header.length);
  appendKnownBytes(state, value, valueBytes);
}

function encodeNumber(value: number, state: EncodingState): void {
  if (!Number.isFinite(value)) return reject("non_finite_number");
  if (Number.isInteger(value) && !Number.isSafeInteger(value)) return reject("unsafe_integer");
  appendFrame(state, "d", Object.is(value, -0) ? "-0" : String(value));
}

function readArrayLength(value: readonly unknown[]): number {
  const descriptor = Reflect.getOwnPropertyDescriptor(value, "length");
  if (
    descriptor === undefined ||
    !("value" in descriptor) ||
    typeof descriptor.value !== "number" ||
    !Number.isSafeInteger(descriptor.value) ||
    descriptor.value < 0
  ) {
    return reject("invalid_array_length");
  }
  return descriptor.value;
}

function encodeArray(value: readonly unknown[], state: EncodingState, depth: number): void {
  if (Reflect.getPrototypeOf(value) !== Array.prototype) return reject("unsupported_array");
  const length = readArrayLength(value);
  if (length > REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH) {
    return rejectCountBudget(
      "replay_fingerprint_container_width",
      REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH,
    );
  }

  const ownKeys = Reflect.ownKeys(value);
  if (ownKeys.some((key) => typeof key === "symbol")) return reject("symbol_key");
  const indexKeys = new Set<string>();
  for (const key of ownKeys as string[]) {
    if (key === "length") continue;
    if (!/^(?:0|[1-9]\d*)$/u.test(key)) return reject("array_extra_property", { key });
    const index = Number(key);
    if (!Number.isSafeInteger(index) || index < 0 || index >= length || String(index) !== key) {
      return reject("array_extra_property", { key });
    }
    indexKeys.add(key);
  }
  if (indexKeys.size !== length) return reject("sparse_array");

  appendAscii(state, `a${String(length)}[`);
  for (let index = 0; index < length; index += 1) {
    const key = String(index);
    const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined) return reject("sparse_array");
    if (!("value" in descriptor) || !descriptor.enumerable) {
      return reject("accessor_property", { key });
    }
    encodeValue(descriptor.value as unknown, state, depth + 1);
  }
  appendAscii(state, "]");
}

function readByteLength(value: Uint8Array): number {
  const byteLength = Reflect.get(Uint8Array.prototype, "byteLength", value) as unknown;
  if (typeof byteLength !== "number" || !Number.isSafeInteger(byteLength) || byteLength < 0) {
    return reject("invalid_byte_array_length");
  }
  return byteLength;
}

function encodeByteArray(value: Uint8Array, state: EncodingState): void {
  const expectedPrototype = Buffer.isBuffer(value) ? BUFFER_PROTOTYPE : Uint8Array.prototype;
  if (Reflect.getPrototypeOf(value) !== expectedPrototype) {
    return reject("unsupported_byte_array");
  }
  const byteLength = readByteLength(value);
  if (byteLength > REPLAY_FINGERPRINT_MAX_BYTE_ARRAY_BYTES) {
    return rejectByteBudget(
      "replay_fingerprint_byte_array_bytes",
      REPLAY_FINGERPRINT_MAX_BYTE_ARRAY_BYTES,
    );
  }
  const header = `y${String(byteLength)}:`;
  ensureEncodedCapacity(state, header.length + byteLength * 2);

  const ownKeys = Reflect.ownKeys(value);
  if (ownKeys.some((key) => typeof key === "symbol")) return reject("symbol_key");
  const keys = ownKeys as string[];
  for (const key of keys) {
    if (!/^(?:0|[1-9]\d*)$/u.test(key)) {
      return reject("byte_array_extra_property", { key });
    }
  }
  if (keys.length !== byteLength) return reject("sparse_byte_array");
  const keySet = new Set(keys);

  const bytes = Buffer.allocUnsafe(byteLength);
  for (let index = 0; index < byteLength; index += 1) {
    const key = String(index);
    if (!keySet.has(key)) return reject("sparse_byte_array", { key });
    const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
    if (
      descriptor === undefined ||
      !("value" in descriptor) ||
      !descriptor.enumerable ||
      typeof descriptor.value !== "number" ||
      !Number.isInteger(descriptor.value) ||
      descriptor.value < 0 ||
      descriptor.value > 255
    ) {
      return reject("invalid_byte_property", { key });
    }
    bytes[index] = descriptor.value;
  }
  appendAscii(state, header);
  appendAscii(state, bytes.toString("hex"));
}

function encodeObject(value: object, state: EncodingState, depth: number): void {
  const prototype = Reflect.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) return reject("unsupported_object");
  const ownKeys = Reflect.ownKeys(value);
  if (ownKeys.length > REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH) {
    return rejectCountBudget(
      "replay_fingerprint_container_width",
      REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH,
    );
  }
  if (ownKeys.some((key) => typeof key === "symbol")) return reject("symbol_key");
  const keys = (ownKeys as string[]).sort();

  appendAscii(state, `o${String(keys.length)}{`);
  for (const key of keys) {
    if (DANGEROUS_KEYS.has(key)) return reject("dangerous_key", { key });
    if (!isWellFormedString(key)) return reject("ill_formed_unicode", { key });
    const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      return reject("accessor_property", { key });
    }
    appendFrame(state, "k", key);
    encodeValue(descriptor.value as unknown, state, depth + 1);
  }
  appendAscii(state, "}");
}

function encodeValue(value: unknown, state: EncodingState, depth: number): void {
  if (depth > REPLAY_FINGERPRINT_MAX_DEPTH) {
    return rejectCountBudget("replay_fingerprint_depth", REPLAY_FINGERPRINT_MAX_DEPTH);
  }
  if (state.nodes >= REPLAY_FINGERPRINT_MAX_NODES) {
    return rejectCountBudget("replay_fingerprint_nodes", REPLAY_FINGERPRINT_MAX_NODES);
  }
  state.nodes += 1;

  if (value === null) {
    appendAscii(state, "n");
    return;
  }
  if (typeof value === "boolean") {
    appendAscii(state, value ? "b1" : "b0");
    return;
  }
  if (typeof value === "string") {
    if (!isWellFormedString(value)) return reject("ill_formed_unicode");
    appendFrame(state, "s", value);
    return;
  }
  if (typeof value === "number") {
    encodeNumber(value, state);
    return;
  }
  if (typeof value === "bigint") {
    appendFrame(state, "i", value.toString(10));
    return;
  }
  if (typeof value !== "object") return reject(`unsupported_${typeof value}`);
  if (nodeTypes.isProxy(value)) return reject("proxy_value");
  if (state.ancestors.has(value)) return reject("cyclic_value");

  state.ancestors.add(value);
  try {
    if (nodeTypes.isUint8Array(value)) {
      encodeByteArray(value, state);
      return;
    }
    if (Array.isArray(value)) encodeArray(value, state, depth);
    else encodeObject(value, state, depth);
  } finally {
    state.ancestors.delete(value);
  }
}

function encodeInto(value: unknown, onChunk: (chunk: string) => void): void {
  encodeValue(value, { ancestors: new WeakSet(), encodedBytes: 0, nodes: 0, onChunk }, 0);
}

export function canonicalEncode(value: unknown): string {
  const chunks: string[] = [];
  encodeInto(value, (chunk) => chunks.push(chunk));
  return chunks.join("");
}

export function createReplayKey(
  clientInstanceId: ClientInstanceId,
  commandId: CommandId,
): ReplayKey {
  const clientLength = String(Buffer.byteLength(clientInstanceId, "utf8"));
  const commandLength = String(Buffer.byteLength(commandId, "utf8"));
  return `${clientLength}:${clientInstanceId}${commandLength}:${commandId}` as ReplayKey;
}

export function commandFingerprint(commandName: string, payload: unknown): CommandFingerprint {
  const validatedCommandName = assertIdentifier(commandName, "commandName");
  const hash = createHash("sha256");
  encodeInto([validatedCommandName, payload], (chunk) => {
    hash.update(chunk, "utf8");
  });
  return hash.digest("hex") as CommandFingerprint;
}
