import { timingSafeEqual } from "node:crypto";

export const AUTHORIZATION_METADATA_KEY = "authorization";

const TOKEN_BYTES = 32;
const TOKEN_LENGTH = 43;
const BASE64_URL_NO_PADDING = /^[A-Za-z0-9_-]{43}$/u;

export type BearerAuthorizationResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly code: "MISSING" | "MALFORMED" | "UNAUTHORIZED" };

export type InvalidBearerTokenCode = "INVALID_BEARER_TOKEN" | "INVALID_EXPECTED_TOKEN";

export class InvalidBearerTokenError extends Error {
  public override readonly name = "InvalidBearerTokenError";

  public constructor(public readonly code: InvalidBearerTokenCode) {
    super("bearer token must be canonical unpadded base64url encoding of 32 bytes");
  }
}

function decodeCanonicalToken(token: unknown): Buffer | undefined {
  if (
    typeof token !== "string" ||
    token.length !== TOKEN_LENGTH ||
    !BASE64_URL_NO_PADDING.test(token)
  ) {
    return undefined;
  }
  const decoded = Buffer.from(token, "base64url");
  if (decoded.length !== TOKEN_BYTES || decoded.toString("base64url") !== token) {
    return undefined;
  }
  return decoded;
}

interface CanonicalToken {
  readonly decoded: Buffer;
  readonly encoded: string;
}

function requireCanonicalToken(token: unknown, code: InvalidBearerTokenCode): CanonicalToken {
  const decoded = decodeCanonicalToken(token);
  if (decoded === undefined || typeof token !== "string") {
    throw new InvalidBearerTokenError(code);
  }
  return { decoded, encoded: token };
}

export function bearerAuthorization(token: unknown): string {
  const canonical = requireCanonicalToken(token, "INVALID_BEARER_TOKEN");
  return `Bearer ${canonical.encoded}`;
}

export function authorizeBearer(
  authorization: unknown,
  expectedToken: unknown,
): BearerAuthorizationResult {
  const expected = requireCanonicalToken(expectedToken, "INVALID_EXPECTED_TOKEN");
  if (authorization === undefined) {
    return { ok: false, code: "MISSING" };
  }
  if (typeof authorization !== "string") {
    return { ok: false, code: "MALFORMED" };
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
  return timingSafeEqual(expected.decoded, candidate)
    ? { ok: true }
    : { ok: false, code: "UNAUTHORIZED" };
}
