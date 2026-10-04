import { status, type ServerErrorResponse } from "@grpc/grpc-js";
import {
  PROTOCOL_VERSION,
  validateStringId,
  validateText,
  type RequestMetadata,
} from "@lyonbrown/protocol";
import { epochMismatchStatus, trustedStatus } from "./status.js";

const SCOPE_FINGERPRINT = /^[a-f0-9]{64}$/u;

function invalidArgument(message: string): ServerErrorResponse {
  return trustedStatus(status.INVALID_ARGUMENT, message);
}

export function validateServerIdentity(serverInstanceId: string, scopeFingerprint: string): void {
  if (!validateStringId(serverInstanceId).ok) throw new TypeError("serverInstanceId is invalid");
  if (!SCOPE_FINGERPRINT.test(scopeFingerprint)) throw new TypeError("scopeFingerprint is invalid");
}

export function requireMetadata(metadata: RequestMetadata | undefined): RequestMetadata {
  if (metadata === undefined) throw invalidArgument("metadata is required");
  if (metadata.protocolVersion !== PROTOCOL_VERSION) {
    const message = `unsupported protocol_version ${String(metadata.protocolVersion)}; server supports protocol_version ${String(PROTOCOL_VERSION)}`;
    throw trustedStatus(status.FAILED_PRECONDITION, message);
  }
  if (!validateStringId(metadata.clientInstanceId).ok) {
    throw invalidArgument("client_instance_id is invalid");
  }
  if (!validateStringId(metadata.commandId).ok) throw invalidArgument("command_id is invalid");
  return metadata;
}

export function requireEpoch(metadata: RequestMetadata, actualServerInstanceId: string): void {
  if (metadata.expectedServerInstanceId !== actualServerInstanceId) {
    throw epochMismatchStatus(metadata.expectedServerInstanceId, actualServerInstanceId);
  }
}

export function requireText(field: string, value: string): void {
  if (value.trim().length === 0) throw invalidArgument(`${field} is required`);
  if (!validateText(value).ok) throw invalidArgument(`${field} is invalid`);
}

export function requireBoundedText(field: string, value: string, maximumBytes: number): void {
  if (value.trim().length === 0) throw invalidArgument(`${field} is required`);
  if (Buffer.byteLength(value, "utf8") > maximumBytes) {
    throw invalidArgument(`${field} exceeds ${String(maximumBytes)} bytes`);
  }
}
