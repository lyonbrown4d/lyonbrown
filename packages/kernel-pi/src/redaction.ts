import { kernelError } from "./errors.js";
import { inspectOwnDataProperties } from "./inspection.js";
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
      throw kernelError(
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
    const redacted = redactJson(
      sanitizeJson(value, limits),
      (text) => this.text(text),
      limits.maxCollectionEntries,
    );
    return sanitizeJson(redacted, limits);
  }
}

function compareSecrets(left: string, right: string): number {
  return utf8Bytes(right) - utf8Bytes(left) || right.length - left.length;
}

function redactJson(
  value: KernelJsonValue,
  redact: (value: string) => string,
  maximumKeys: number,
): KernelJsonValue {
  if (typeof value === "string") return redact(value);
  if (Array.isArray(value)) {
    const entries = value as readonly KernelJsonValue[];
    return entries.map((entry) => redactJson(entry, redact, maximumKeys));
  }
  if (value !== null && typeof value === "object") {
    const output = Object.create(null) as Record<string, KernelJsonValue>;
    const properties = inspectOwnDataProperties(value, maximumKeys, () =>
      kernelError("UNTRUSTED_VALUE_INVALID", "Redaction input inspection failed"),
    );
    for (const property of properties) {
      if (!property.enumerable) continue;
      const redactedKey = redact(property.key);
      if (Object.hasOwn(output, redactedKey)) {
        throw kernelError("UNTRUSTED_VALUE_INVALID", "Redacted object keys collide");
      }
      output[redactedKey] = redactJson(property.value as KernelJsonValue, redact, maximumKeys);
    }
    return output;
  }
  return value;
}
