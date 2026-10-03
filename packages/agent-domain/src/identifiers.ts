import { domainError } from "./errors.js";
import { assertWellFormedString } from "./unicode.js";

declare const sessionIdBrand: unique symbol;
declare const turnIdBrand: unique symbol;
declare const commandIdBrand: unique symbol;
declare const clientInstanceIdBrand: unique symbol;
declare const serverInstanceIdBrand: unique symbol;
declare const approvalIdBrand: unique symbol;
declare const modelIdBrand: unique symbol;

export type SessionId = string & { readonly [sessionIdBrand]: true };
export type TurnId = string & { readonly [turnIdBrand]: true };
export type CommandId = string & { readonly [commandIdBrand]: true };
export type ClientInstanceId = string & { readonly [clientInstanceIdBrand]: true };
export type ServerInstanceId = string & { readonly [serverInstanceIdBrand]: true };
export type ApprovalId = string & { readonly [approvalIdBrand]: true };
export type ModelId = string & { readonly [modelIdBrand]: true };

export const MAX_IDENTIFIER_BYTES = 128;

function validateIdentifier(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw domainError("invalid_argument", `${field} must be a string`, {
      field,
      reason: "invalid_type",
    });
  }
  if (value.length === 0) {
    throw domainError("invalid_argument", `${field} must not be empty`, {
      field,
      reason: "empty",
    });
  }
  assertWellFormedString(value, field);
  if (Buffer.byteLength(value, "utf8") > MAX_IDENTIFIER_BYTES) {
    throw domainError("invalid_argument", `${field} exceeds the UTF-8 byte limit`, {
      field,
      reason: "too_large",
      limitBytes: MAX_IDENTIFIER_BYTES,
    });
  }
  return value;
}

export function asSessionId(value: unknown): SessionId {
  return validateIdentifier(value, "sessionId") as SessionId;
}

export function asTurnId(value: unknown): TurnId {
  return validateIdentifier(value, "turnId") as TurnId;
}

export function asCommandId(value: unknown): CommandId {
  return validateIdentifier(value, "commandId") as CommandId;
}

export function asClientInstanceId(value: unknown): ClientInstanceId {
  return validateIdentifier(value, "clientInstanceId") as ClientInstanceId;
}

export function asServerInstanceId(value: unknown): ServerInstanceId {
  return validateIdentifier(value, "serverInstanceId") as ServerInstanceId;
}

export function asApprovalId(value: unknown): ApprovalId {
  return validateIdentifier(value, "approvalId") as ApprovalId;
}

export function asModelId(value: unknown): ModelId {
  return validateIdentifier(value, "modelId") as ModelId;
}

export function assertIdentifier(value: unknown, field: string): string {
  return validateIdentifier(value, field);
}
