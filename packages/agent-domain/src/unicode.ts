import { domainError } from "./errors.ts";

interface WellFormedStringPrototype {
  isWellFormed(this: string): boolean;
}

const stringPrototype = String.prototype as unknown as WellFormedStringPrototype;

export function isWellFormedString(value: string): boolean {
  return stringPrototype.isWellFormed.call(value);
}

export function assertWellFormedString(value: string, field: string): string {
  if (!isWellFormedString(value)) {
    throw domainError("invalid_argument", `${field} contains ill-formed Unicode`, {
      field,
      reason: "ill_formed_unicode",
    });
  }
  return value;
}
