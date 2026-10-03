import { timingSafeEqual } from "node:crypto";

export const AUTHORIZATION_METADATA_KEY = "authorization";

const TOKEN_BYTES = 32;
const TOKEN_LENGTH = 43;
const BASE64_URL_NO_PADDING = /^[A-Za-z0-9_-]{43}$/u;

export type BearerAuthorizationResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly code: "MISSING" | "MALFORMED" | "UNAUTHORIZED" };

function decodeCanonicalToken(token: string): Buffer | undefined {
  if (token.length !== TOKEN_LENGTH || !BASE64_URL_NO_PADDING.test(token)) {
    return undefined;
  }
  const decoded = Buffer.from(token, "base64url");
  if (decoded.length !== TOKEN_BYTES || decoded.toString("base64url") !== token) {
    return undefined;
  }
  return decoded;
}

function requireCanonicalToken(token: string): Buffer {
  const decoded = decodeCanonicalToken(token);
  if (decoded === undefined) {
    throw new TypeError("bearer token must be canonical unpadded base64url encoding of 32 bytes");
  }
  return decoded;
}

export function bearerAuthorization(token: string): string {
  requireCanonicalToken(token);
  return `Bearer ${token}`;
}

export function authorizeBearer(
  authorization: string | undefined,
  expectedToken: string,
): BearerAuthorizationResult {
  const expected = requireCanonicalToken(expectedToken);
  if (authorization === undefined) {
    return { ok: false, code: "MISSING" };
  }

  const separator = authorization.indexOf(" ");
  if (separator === -1) {
    return { ok: false, code: "MALFORMED" };
  }
  const scheme = authorization.slice(0, separator);
  const encoded = authorization.slice(separator + 1);
  if (scheme.toLowerCase() !== "bearer" || encoded.length === 0 || /\s/u.test(encoded)) {
    return { ok: false, code: "MALFORMED" };
  }

  const candidate = decodeCanonicalToken(encoded);
  if (candidate === undefined) {
    return { ok: false, code: "MALFORMED" };
  }
  return timingSafeEqual(expected, candidate) ? { ok: true } : { ok: false, code: "UNAUTHORIZED" };
}
