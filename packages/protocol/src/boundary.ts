export const MAX_STRING_ID_BYTES = 128;
export const MAX_TEXT_BYTES = 1024 * 1024;

export type BoundaryValidationCode = "STRING_ID_EMPTY" | "STRING_ID_TOO_LARGE" | "TEXT_TOO_LARGE";

export type BoundaryValidationResult =
  | { readonly ok: true; readonly bytes: number }
  | {
      readonly ok: false;
      readonly code: BoundaryValidationCode;
      readonly bytes: number;
      readonly limit: number;
    };

export function validateStringId(value: string): BoundaryValidationResult {
  const bytes = Buffer.byteLength(value, "utf8");
  if (bytes === 0) {
    return { ok: false, code: "STRING_ID_EMPTY", bytes, limit: MAX_STRING_ID_BYTES };
  }
  if (bytes > MAX_STRING_ID_BYTES) {
    return { ok: false, code: "STRING_ID_TOO_LARGE", bytes, limit: MAX_STRING_ID_BYTES };
  }
  return { ok: true, bytes };
}

export function validateText(value: string): BoundaryValidationResult {
  const bytes = Buffer.byteLength(value, "utf8");
  if (bytes > MAX_TEXT_BYTES) {
    return { ok: false, code: "TEXT_TOO_LARGE", bytes, limit: MAX_TEXT_BYTES };
  }
  return { ok: true, bytes };
}
