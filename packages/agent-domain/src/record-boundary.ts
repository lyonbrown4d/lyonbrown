import { types as nodeTypes } from "node:util";

import { domainError } from "./errors.ts";

const DANGEROUS_KEYS = new Set(["__proto__", "prototype", "constructor"]);

function invalidRecord(field: string, reason: string, key?: string): never {
  throw domainError("invalid_argument", `${field} is not a safe data record`, {
    field: key === undefined ? field : `${field}.${key}`,
    reason,
    ...(key === undefined ? {} : { key }),
  });
}

export function readDataRecord(
  value: unknown,
  field: string,
  allowedFields: ReadonlySet<string>,
): ReadonlyMap<string, unknown> {
  if (value === null || typeof value !== "object") return invalidRecord(field, "invalid_record");
  if (nodeTypes.isProxy(value)) return invalidRecord(field, "proxy_record");
  if (Array.isArray(value)) return invalidRecord(field, "invalid_record");

  const prototype = Reflect.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    return invalidRecord(field, "invalid_record_prototype");
  }

  const keys = Reflect.ownKeys(value);
  const fields = new Map<string, unknown>();
  for (const key of keys) {
    if (typeof key !== "string") return invalidRecord(field, "symbol_property");
    if (DANGEROUS_KEYS.has(key)) return invalidRecord(field, "dangerous_property", key);
    if (!allowedFields.has(key)) return invalidRecord(field, "unexpected_field", key);
    const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined || !("value" in descriptor)) {
      return invalidRecord(field, "accessor_property", key);
    }
    if (!descriptor.enumerable) return invalidRecord(field, "non_enumerable_property", key);
    fields.set(key, descriptor.value as unknown);
  }
  return fields;
}

export function requireDataField(
  record: ReadonlyMap<string, unknown>,
  recordName: string,
  field: string,
): unknown {
  if (!record.has(field)) {
    throw domainError("invalid_argument", `${recordName}.${field} is required`, {
      field: `${recordName}.${field}`,
      reason: "missing_required_field",
    });
  }
  return record.get(field);
}
