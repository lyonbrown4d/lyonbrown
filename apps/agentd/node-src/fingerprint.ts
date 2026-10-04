import { createHash } from "node:crypto";

export function commandFingerprint(kind: string, fields: readonly string[]): string {
  const digest = createHash("sha256");
  for (const field of [kind, ...fields]) {
    const bytes = Buffer.from(field, "utf8");
    const length = Buffer.alloc(8);
    length.writeBigUInt64BE(BigInt(bytes.length));
    digest.update(length).update(bytes);
  }
  return digest.digest("base64url");
}
