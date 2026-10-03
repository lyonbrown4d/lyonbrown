export const MAX_STRING_ID_BYTES = 128;
export const MAX_TEXT_BYTES = 1024 * 1024;

export type BoundaryValidationCode =
  | "STRING_REQUIRED"
  | "STRING_NOT_WELL_FORMED"
  | "STRING_ID_EMPTY"
  | "STRING_ID_TOO_LARGE"
  | "TEXT_TOO_LARGE";

export type BoundaryValidationResult =
  | { readonly ok: true; readonly bytes: number }
  | {
      readonly ok: false;
      readonly code: "STRING_REQUIRED" | "STRING_NOT_WELL_FORMED";
    }
  | {
      readonly ok: false;
      readonly code: "STRING_ID_EMPTY" | "STRING_ID_TOO_LARGE" | "TEXT_TOO_LARGE";
      readonly bytes: number;
      readonly limit: number;
    };

function isWellFormedString(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const codeUnit = value.charCodeAt(index);
    if (codeUnit >= 0xd800 && codeUnit <= 0xdbff) {
      if (index + 1 >= value.length) {
        return false;
      }
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) {
        return false;
      }
      index += 1;
    } else if (codeUnit >= 0xdc00 && codeUnit <= 0xdfff) {
      return false;
    }
  }
  return true;
}

type PrimitiveStringValidation =
  | { readonly ok: true; readonly value: string }
  | {
      readonly ok: false;
      readonly code: "STRING_REQUIRED" | "STRING_NOT_WELL_FORMED";
    };

function validatePrimitiveString(value: unknown): PrimitiveStringValidation {
  if (typeof value !== "string") {
    return { ok: false, code: "STRING_REQUIRED" };
  }
  if (!isWellFormedString(value)) {
    return { ok: false, code: "STRING_NOT_WELL_FORMED" };
  }
  return { ok: true, value };
}

export function validateStringId(value: unknown): BoundaryValidationResult {
  const validated = validatePrimitiveString(value);
  if (!validated.ok) {
    return validated;
  }

  const bytes = Buffer.byteLength(validated.value, "utf8");
  if (bytes === 0) {
    return { ok: false, code: "STRING_ID_EMPTY", bytes, limit: MAX_STRING_ID_BYTES };
  }
  if (bytes > MAX_STRING_ID_BYTES) {
    return { ok: false, code: "STRING_ID_TOO_LARGE", bytes, limit: MAX_STRING_ID_BYTES };
  }
  return { ok: true, bytes };
}

export function validateText(value: unknown): BoundaryValidationResult {
  const validated = validatePrimitiveString(value);
  if (!validated.ok) {
    return validated;
  }

  const bytes = Buffer.byteLength(validated.value, "utf8");
  if (bytes > MAX_TEXT_BYTES) {
    return { ok: false, code: "TEXT_TOO_LARGE", bytes, limit: MAX_TEXT_BYTES };
  }
  return { ok: true, bytes };
}
