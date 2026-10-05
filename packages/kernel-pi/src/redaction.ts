import { kernelError } from "./errors.ts";
import { inspectOwnDataProperties } from "./inspection.ts";
import {
  boundedSummary,
  sanitizeJson,
  sanitizeText,
  utf8Bytes,
  type SanitizationLimits,
} from "./sanitization.ts";
import type { KernelJsonValue } from "./types.ts";

export interface SecretRedactorOptions {
  readonly maxSecretCount: number;
  readonly maxSecretBytes: number;
}

const DEFAULT_LIMITS: SecretRedactorOptions = {
  maxSecretCount: 128,
  maxSecretBytes: 65_536,
};

export class SecretRedactor {
  readonly #secrets = new Set<string>();
  #sortedSecrets: readonly string[] = [];
  #secretBytes = 0;

  constructor(private readonly options: SecretRedactorOptions = DEFAULT_LIMITS) {
    if (
      !Number.isSafeInteger(options.maxSecretCount) ||
      options.maxSecretCount <= 0 ||
      !Number.isSafeInteger(options.maxSecretBytes) ||
      options.maxSecretBytes <= 0
    ) {
      throw new TypeError("Secret redaction limits must be positive safe integers");
    }
  }

  add(secret: string): void {
    const normalized = sanitizeText(secret);
    if (secret.length === 0 || normalized.length === 0) {
      throw kernelError(
        "UNTRUSTED_VALUE_INVALID",
        "Secret value cannot be empty after normalization",
      );
    }
    const additions = [...new Set([secret, normalized])].filter(
      (candidate) => !this.#secrets.has(candidate),
    );
    const addedBytes = additions.reduce((total, candidate) => total + utf8Bytes(candidate), 0);
    if (
      additions.length > this.options.maxSecretCount - this.#secrets.size ||
      addedBytes > this.options.maxSecretBytes - this.#secretBytes
    ) {
      throw kernelError("INPUT_BUDGET_EXCEEDED", "Secret redaction resource budget exceeded");
    }
    if (additions.length === 0) return;
    for (const candidate of additions) this.#secrets.add(candidate);
    this.#secretBytes += addedBytes;
    this.#sortedSecrets = [...this.#secrets].sort(compareSecrets);
  }

  text(value: string): string {
    let output = sanitizeText(value);
    for (const secret of this.#sortedSecrets) {
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
