import { domainError } from "@lyonbrown/agent-domain/internal/errors";

declare const revisionBrand: unique symbol;

export type Revision = bigint & { readonly [revisionBrand]: true };

export const MAX_UINT64 = 18_446_744_073_709_551_615n;

export function asRevision(value: unknown): Revision {
  if (typeof value !== "bigint" || value < 0n || value > MAX_UINT64) {
    throw domainError("invalid_argument", "Revision must be an unsigned 64-bit bigint", {
      field: "revision",
      reason: "invalid_uint64",
    });
  }
  return value as Revision;
}

export function incrementRevision(value: Revision): Revision {
  if (value === MAX_UINT64) {
    throw domainError("resource_exhausted", "Revision exceeds the uint64 range", {
      resource: "revision",
      limit: MAX_UINT64,
    });
  }
  return asRevision(value + 1n);
}
