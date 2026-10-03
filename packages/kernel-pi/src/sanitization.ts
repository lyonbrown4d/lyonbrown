import { stripVTControlCharacters } from "node:util";

import { KernelRuntimeError } from "./errors.js";
import type { KernelJsonValue } from "./types.js";

// eslint-disable-next-line no-control-regex -- the sanitizer intentionally removes C0/C1 controls
const CONTROL_CHARACTERS = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/gu;
const BIDI_CONTROLS = /[\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]/gu;
const DANGEROUS_KEYS = new Set(["__proto__", "constructor", "prototype"]);

export function sanitizeText(value: string): string {
  return stripVTControlCharacters(toWellFormed(value))
    .replace(CONTROL_CHARACTERS, "")
    .replace(BIDI_CONTROLS, "");
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

export function assertByteLimit(
  value: string,
  maximum: number,
  code: ConstructorParameters<typeof KernelRuntimeError>[0],
): void {
  if (utf8Bytes(value) > maximum) {
    throw new KernelRuntimeError(code, `Value exceeds the ${String(maximum)}-byte resource budget`);
  }
}

export function sanitizeJson(value: unknown, maximumDepth: number, depth = 0): KernelJsonValue {
  if (depth > maximumDepth) {
    throw new KernelRuntimeError(
      "UNTRUSTED_VALUE_INVALID",
      "Value exceeds the allowed nesting depth",
    );
  }
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") return sanitizeText(value);
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (Array.isArray(value)) {
    return value.map((entry) => sanitizeJson(entry, maximumDepth, depth + 1));
  }
  if (typeof value === "object") {
    const prototype = Object.getPrototypeOf(value) as object | null;
    if (prototype !== Object.prototype && prototype !== null) {
      throw new KernelRuntimeError(
        "UNTRUSTED_VALUE_INVALID",
        "Only plain JSON objects are accepted",
      );
    }
    const sanitized = Object.create(null) as Record<string, KernelJsonValue>;
    for (const [key, entry] of Object.entries(value)) {
      if (DANGEROUS_KEYS.has(key)) {
        throw new KernelRuntimeError("UNTRUSTED_VALUE_INVALID", "Dangerous object key rejected");
      }
      sanitized[sanitizeText(key)] = sanitizeJson(entry, maximumDepth, depth + 1);
    }
    return sanitized;
  }
  throw new KernelRuntimeError("UNTRUSTED_VALUE_INVALID", "Value is not JSON-compatible");
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

  json(value: unknown, maximumDepth: number): KernelJsonValue {
    return redactJson(sanitizeJson(value, maximumDepth), (text) => this.text(text));
  }
}

function redactJson(value: KernelJsonValue, redact: (value: string) => string): KernelJsonValue {
  if (typeof value === "string") return redact(value);
  if (Array.isArray(value)) {
    const entries = value as readonly KernelJsonValue[];
    return entries.map((entry) => redactJson(entry, redact));
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, redactJson(entry, redact)]),
    );
  }
  return value;
}

export function redactSecretReferences(
  value: unknown,
  maximumDepth: number,
  depth = 0,
): KernelJsonValue {
  if (depth > maximumDepth) {
    throw new KernelRuntimeError(
      "UNTRUSTED_VALUE_INVALID",
      "Value exceeds the allowed nesting depth",
    );
  }
  if (isSecretReference(value)) return "[REDACTED]";
  if (Array.isArray(value))
    return value.map((entry) => redactSecretReferences(entry, maximumDepth, depth + 1));
  if (typeof value === "object" && value !== null) {
    const result = Object.create(null) as Record<string, KernelJsonValue>;
    for (const [key, entry] of Object.entries(value)) {
      if (DANGEROUS_KEYS.has(key)) {
        throw new KernelRuntimeError("UNTRUSTED_VALUE_INVALID", "Dangerous object key rejected");
      }
      result[sanitizeText(key)] = redactSecretReferences(entry, maximumDepth, depth + 1);
    }
    return result;
  }
  return sanitizeJson(value, maximumDepth, depth);
}

export function isSecretReference(
  value: unknown,
): value is { kind: "secretRef"; reference: string } {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const candidate = value as Record<string, unknown>;
  return candidate.kind === "secretRef" && typeof candidate.reference === "string";
}
