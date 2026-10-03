import { KernelRuntimeError } from "./errors.js";
import {
  boundedSummary,
  sanitizeJson,
  sanitizeText,
  utf8Bytes,
  type SanitizationLimits,
} from "./sanitization.js";
import type { KernelJsonValue } from "./types.js";

export class SecretRedactor {
  readonly #secrets = new Set<string>();

  add(secret: string): void {
    const normalized = sanitizeText(secret);
    if (secret.length === 0 || normalized.length === 0) {
      throw new KernelRuntimeError(
        "UNTRUSTED_VALUE_INVALID",
        "Secret value cannot be empty after normalization",
      );
    }
    this.#secrets.add(secret);
    this.#secrets.add(normalized);
  }

  text(value: string): string {
    let output = sanitizeText(value);
    for (const secret of [...this.#secrets].sort(compareSecrets)) {
      output = output.replaceAll(secret, "[REDACTED]");
    }
    return output;
  }

  summary(value: string, maximumBytes = 256): string {
    return boundedSummary(this.text(value), maximumBytes);
  }

  json(value: unknown, limits: SanitizationLimits): KernelJsonValue {
    const redacted = redactJson(sanitizeJson(value, limits), (text) => this.text(text));
    return sanitizeJson(redacted, limits);
  }
}

function compareSecrets(left: string, right: string): number {
  return utf8Bytes(right) - utf8Bytes(left) || right.length - left.length;
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
        throw new KernelRuntimeError("UNTRUSTED_VALUE_INVALID", "Redacted object keys collide");
      }
      output[redactedKey] = redactJson(entry, redact);
    }
    return output;
  }
  return value;
}
