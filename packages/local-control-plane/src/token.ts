import { randomBytes, timingSafeEqual } from "node:crypto";
import { inspect } from "node:util";
import { controlPlaneError } from "./errors.js";

const TOKEN_BYTES = 32;
const TOKEN_PATTERN = /^[A-Za-z0-9_-]{43}$/u;

function decodeCanonicalToken(encoded: string): Buffer | undefined {
  if (!TOKEN_PATTERN.test(encoded)) return undefined;
  const decoded = Buffer.from(encoded, "base64url");
  if (decoded.length !== TOKEN_BYTES || decoded.toString("base64url") !== encoded) return undefined;
  return decoded;
}

export class SecretToken {
  readonly #encoded: string;
  readonly #bytes: Buffer;

  private constructor(encoded: string, bytes: Buffer) {
    this.#encoded = encoded;
    this.#bytes = bytes;
    Object.freeze(this);
  }

  public static generate(): SecretToken {
    return SecretToken.fromBytes(randomBytes(TOKEN_BYTES));
  }

  public static parse(encoded: string): SecretToken {
    const bytes = decodeCanonicalToken(encoded);
    if (bytes === undefined) throw controlPlaneError("INVALID_TOKEN");
    return new SecretToken(encoded, bytes);
  }

  private static fromBytes(bytes: Buffer): SecretToken {
    return new SecretToken(bytes.toString("base64url"), Buffer.from(bytes));
  }

  public exposeForTransport(): string {
    return this.#encoded;
  }

  public equals(candidate: SecretToken): boolean {
    return timingSafeEqual(this.#bytes, candidate.#bytes);
  }

  public toString(): string {
    return "[REDACTED]";
  }

  public toJSON(): string {
    return "[REDACTED]";
  }

  public [inspect.custom](): string {
    return "SecretToken([REDACTED])";
  }
}

export function verifySecretToken(expected: SecretToken, candidate: SecretToken): boolean {
  return expected.equals(candidate);
}
