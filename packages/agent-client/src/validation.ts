import { validateStringId, validateText } from "@lyonbrown/protocol";

import { clientError } from "./errors.js";

export function requireId(field: string, value: unknown): string {
  const result = validateStringId(value);
  if (!result.ok) {
    throw clientError("validation", "INVALID_ARGUMENT", `${field} is invalid`, {
      field,
      reason: result.code,
    });
  }
  return value as string;
}

export function requireText(field: string, value: unknown): string {
  const result = validateText(value);
  if (!result.ok) {
    throw clientError("validation", "INVALID_ARGUMENT", `${field} is invalid`, {
      field,
      reason: result.code,
    });
  }
  return value as string;
}

export function requireNonNegativeRevision(field: string, value: bigint): bigint {
  if (value < 0n || value > 0xffff_ffff_ffff_ffffn) {
    throw clientError("protocol", "INVALID_RESPONSE", `${field} is outside uint64 range`);
  }
  return value;
}

export function requireJson(field: string, value: unknown): string {
  const text = requireText(field, value);
  try {
    JSON.parse(text);
  } catch {
    throw clientError("protocol", "INVALID_SNAPSHOT", `${field} is not valid JSON`);
  }
  return text;
}

export function protocolId(field: string, value: unknown): string {
  try {
    return requireId(field, value);
  } catch {
    throw clientError("protocol", "INVALID_SNAPSHOT", `${field} is invalid`);
  }
}

export function protocolText(field: string, value: unknown): string {
  try {
    return requireText(field, value);
  } catch {
    throw clientError("protocol", "INVALID_SNAPSHOT", `${field} is invalid`);
  }
}

export function requiredField<T>(field: string, value: T | undefined): T {
  if (value === undefined) {
    throw clientError("protocol", "INVALID_SNAPSHOT", `${field} is missing`);
  }
  return value;
}
