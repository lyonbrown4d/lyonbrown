import { createHash } from "node:crypto";
import { types as nodeTypes } from "node:util";

import { domainError } from "./errors.js";
import { assertIdentifier, type ClientInstanceId, type CommandId } from "./identifiers.js";
import { isWellFormedString } from "./unicode.js";

declare const replayKeyBrand: unique symbol;
declare const commandFingerprintBrand: unique symbol;

export type ReplayKey = string & { readonly [replayKeyBrand]: true };
export type CommandFingerprint = string & { readonly [commandFingerprintBrand]: true };

const DANGEROUS_KEYS = new Set(["__proto__", "prototype", "constructor"]);
const BUFFER_PROTOTYPE = Reflect.getPrototypeOf(Buffer.alloc(0));

function frame(tag: string, value: string): string {
  return `${tag}${String(Buffer.byteLength(value, "utf8"))}:${value}`;
}

function reject(reason: string, extra: { key?: string } = {}): never {
  throw domainError("invalid_argument", `Value cannot be canonically encoded: ${reason}`, {
    field: "value",
    reason,
    ...extra,
  });
}

function encodeNumber(value: number): string {
  if (!Number.isFinite(value)) {
    return reject("non_finite_number");
  }
  if (Number.isInteger(value) && !Number.isSafeInteger(value)) {
    return reject("unsafe_integer");
  }
  const encoded = Object.is(value, -0) ? "-0" : String(value);
  return frame("d", encoded);
}

function encodeArray(value: readonly unknown[], ancestors: WeakSet<object>): string {
  if (Object.getPrototypeOf(value) !== Array.prototype) {
    return reject("unsupported_array");
  }
  const ownKeys = Reflect.ownKeys(value);
  if (ownKeys.some((key) => typeof key === "symbol")) {
    return reject("symbol_key");
  }
  const lengthDescriptor = Object.getOwnPropertyDescriptor(value, "length");
  if (
    lengthDescriptor === undefined ||
    !("value" in lengthDescriptor) ||
    typeof lengthDescriptor.value !== "number" ||
    !Number.isSafeInteger(lengthDescriptor.value) ||
    lengthDescriptor.value < 0
  ) {
    return reject("invalid_array_length");
  }
  const length = lengthDescriptor.value;
  const indexKeys = new Set<string>();
  for (const key of ownKeys as string[]) {
    if (key === "length") continue;
    if (!/^(?:0|[1-9]\d*)$/u.test(key)) {
      return reject("array_extra_property", { key });
    }
    const index = Number(key);
    if (!Number.isSafeInteger(index) || index < 0 || index >= length || String(index) !== key) {
      return reject("array_extra_property", { key });
    }
    indexKeys.add(key);
  }
  if (indexKeys.size !== length) {
    return reject("sparse_array");
  }
  const entries: string[] = [];
  for (let index = 0; index < length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
    if (descriptor === undefined) {
      return reject("sparse_array");
    }
    if (!("value" in descriptor) || !descriptor.enumerable) {
      return reject("accessor_property", { key: String(index) });
    }
    entries.push(encodeValue(descriptor.value, ancestors));
  }
  return `a${String(length)}[${entries.join("")}]`;
}

function encodeByteArray(value: Uint8Array): string {
  const expectedPrototype = Buffer.isBuffer(value) ? BUFFER_PROTOTYPE : Uint8Array.prototype;
  if (Reflect.getPrototypeOf(value) !== expectedPrototype) {
    return reject("unsupported_byte_array");
  }
  const ownKeys = Reflect.ownKeys(value);
  if (ownKeys.some((key) => typeof key === "symbol")) {
    return reject("symbol_key");
  }
  const keys = ownKeys as string[];
  const keySet = new Set(keys);
  for (const key of keys) {
    if (!/^(?:0|[1-9]\d*)$/u.test(key)) {
      return reject("byte_array_extra_property", { key });
    }
  }
  const bytes: string[] = [];
  for (let index = 0; index < keys.length; index += 1) {
    const key = String(index);
    if (!keySet.has(key)) {
      return reject("sparse_byte_array", { key });
    }
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
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
    bytes.push(descriptor.value.toString(16).padStart(2, "0"));
  }
  return `y${String(keys.length)}:${bytes.join("")}`;
}

function encodeObject(value: object, ancestors: WeakSet<object>): string {
  const prototype = Object.getPrototypeOf(value) as unknown;
  if (prototype !== Object.prototype && prototype !== null) {
    return reject("unsupported_object");
  }
  const ownKeys = Reflect.ownKeys(value);
  if (ownKeys.some((key) => typeof key === "symbol")) {
    return reject("symbol_key");
  }
  const keys = (ownKeys as string[]).sort();
  const entries: string[] = [];
  for (const key of keys) {
    if (DANGEROUS_KEYS.has(key)) {
      return reject("dangerous_key", { key });
    }
    if (!isWellFormedString(key)) {
      return reject("ill_formed_unicode", { key });
    }
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      return reject("accessor_property", { key });
    }
    entries.push(`${frame("k", key)}${encodeValue(descriptor.value, ancestors)}`);
  }
  return `o${String(keys.length)}{${entries.join("")}}`;
}

function encodeValue(value: unknown, ancestors: WeakSet<object>): string {
  if (value === null) return "n";
  if (typeof value === "boolean") return value ? "b1" : "b0";
  if (typeof value === "string") {
    return isWellFormedString(value) ? frame("s", value) : reject("ill_formed_unicode");
  }
  if (typeof value === "number") return encodeNumber(value);
  if (typeof value === "bigint") return frame("i", value.toString(10));
  if (typeof value !== "object") return reject(`unsupported_${typeof value}`);
  if (nodeTypes.isProxy(value)) return reject("proxy_value");
  if (ancestors.has(value)) return reject("cyclic_value");

  ancestors.add(value);
  try {
    if (nodeTypes.isUint8Array(value)) return encodeByteArray(value);
    return Array.isArray(value) ? encodeArray(value, ancestors) : encodeObject(value, ancestors);
  } finally {
    ancestors.delete(value);
  }
}

export function canonicalEncode(value: unknown): string {
  return encodeValue(value, new WeakSet());
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
  const canonical = canonicalEncode([validatedCommandName, payload]);
  return createHash("sha256").update(canonical, "utf8").digest("hex") as CommandFingerprint;
}
