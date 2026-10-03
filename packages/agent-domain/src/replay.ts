import { createHash } from "node:crypto";

import { domainError } from "./errors.js";
import { assertIdentifier, type ClientInstanceId, type CommandId } from "./identifiers.js";

declare const replayKeyBrand: unique symbol;
declare const commandFingerprintBrand: unique symbol;

export type ReplayKey = string & { readonly [replayKeyBrand]: true };
export type CommandFingerprint = string & { readonly [commandFingerprintBrand]: true };

const DANGEROUS_KEYS = new Set(["__proto__", "prototype", "constructor"]);

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
  const ownKeys = Reflect.ownKeys(value);
  if (ownKeys.some((key) => typeof key === "symbol")) {
    return reject("symbol_key");
  }
  const expectedKeys = new Set(["length", ...value.map((_entry, index) => String(index))]);
  if (ownKeys.some((key) => typeof key === "string" && !expectedKeys.has(key))) {
    return reject("array_extra_property");
  }
  const entries: string[] = [];
  for (let index = 0; index < value.length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
    if (descriptor === undefined) {
      return reject("sparse_array");
    }
    if (!("value" in descriptor)) {
      return reject("accessor_property");
    }
    entries.push(encodeValue(descriptor.value, ancestors));
  }
  return `a${String(value.length)}[${entries.join("")}]`;
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
  if (typeof value === "string") return frame("s", value);
  if (typeof value === "number") return encodeNumber(value);
  if (typeof value === "bigint") return frame("i", value.toString(10));
  if (typeof value !== "object") return reject(`unsupported_${typeof value}`);
  if (ancestors.has(value)) return reject("cyclic_value");

  ancestors.add(value);
  try {
    if (value instanceof Uint8Array) {
      return `y${String(value.byteLength)}:${Buffer.from(value).toString("hex")}`;
    }
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
