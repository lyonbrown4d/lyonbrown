import { domainError } from "./errors.js";

export const MAX_TEXT_BYTES = 1024 * 1024;

export function validateText(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw domainError("invalid_argument", `${field} must be a string`, {
      field,
      reason: "invalid_type",
    });
  }
  if (Buffer.byteLength(value, "utf8") > MAX_TEXT_BYTES) {
    throw domainError("resource_exhausted", `${field} exceeds the UTF-8 byte limit`, {
      resource: field,
      limitBytes: MAX_TEXT_BYTES,
    });
  }
  return value;
}
