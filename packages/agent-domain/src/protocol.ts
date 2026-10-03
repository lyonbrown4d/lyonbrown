import { domainError } from "./errors.js";
import {
  asClientInstanceId,
  asCommandId,
  asServerInstanceId,
  type ClientInstanceId,
  type CommandId,
  type ServerInstanceId,
} from "./identifiers.js";

export const PROTOCOL_VERSION = 1;

export interface CommandMetadata {
  readonly protocolVersion: typeof PROTOCOL_VERSION;
  readonly clientInstanceId: ClientInstanceId;
  readonly commandId: CommandId;
  readonly expectedServerInstanceId?: ServerInstanceId;
}

export interface CommandReceipt {
  readonly accepted: boolean;
  readonly protocolVersion: typeof PROTOCOL_VERSION;
  readonly commandId: CommandId;
}

export type CommandMetadataInput = Omit<CommandMetadata, "protocolVersion">;

export interface UntrustedCommandMetadata {
  readonly protocolVersion: unknown;
  readonly clientInstanceId: unknown;
  readonly commandId: unknown;
  readonly expectedServerInstanceId?: unknown;
}

export function createCommandMetadata(input: CommandMetadataInput): CommandMetadata {
  return input.expectedServerInstanceId === undefined
    ? {
        protocolVersion: PROTOCOL_VERSION,
        clientInstanceId: input.clientInstanceId,
        commandId: input.commandId,
      }
    : {
        protocolVersion: PROTOCOL_VERSION,
        clientInstanceId: input.clientInstanceId,
        commandId: input.commandId,
        expectedServerInstanceId: input.expectedServerInstanceId,
      };
}

export function validateCommandMetadata(value: UntrustedCommandMetadata): CommandMetadata {
  if (value.protocolVersion !== PROTOCOL_VERSION) {
    throw domainError("failed_precondition", "Unsupported protocol version", {
      reason: "protocol_version_mismatch",
      expected: PROTOCOL_VERSION,
      ...(typeof value.protocolVersion === "number" || typeof value.protocolVersion === "string"
        ? { actual: value.protocolVersion }
        : {}),
    });
  }
  const input: CommandMetadataInput = {
    clientInstanceId: asClientInstanceId(value.clientInstanceId),
    commandId: asCommandId(value.commandId),
    ...(value.expectedServerInstanceId === undefined
      ? {}
      : { expectedServerInstanceId: asServerInstanceId(value.expectedServerInstanceId) }),
  };
  return createCommandMetadata(input);
}

export function createCommandReceipt(commandId: CommandId, accepted: boolean): CommandReceipt {
  return { accepted, protocolVersion: PROTOCOL_VERSION, commandId };
}

export function validateCommandReceipt(
  value: Readonly<{ accepted: unknown; protocolVersion: unknown; commandId: unknown }>,
  expectedCommandId?: CommandId,
): CommandReceipt {
  if (value.protocolVersion !== PROTOCOL_VERSION) {
    throw domainError("failed_precondition", "Unsupported protocol version", {
      reason: "protocol_version_mismatch",
      expected: PROTOCOL_VERSION,
      ...(typeof value.protocolVersion === "number" || typeof value.protocolVersion === "string"
        ? { actual: value.protocolVersion }
        : {}),
    });
  }
  if (typeof value.accepted !== "boolean") {
    throw domainError("invalid_argument", "Receipt accepted flag must be boolean", {
      field: "accepted",
      reason: "invalid_type",
    });
  }
  const commandId = asCommandId(value.commandId);
  if (expectedCommandId !== undefined && commandId !== expectedCommandId) {
    throw domainError("failed_precondition", "Receipt belongs to a different command", {
      reason: "command_id_mismatch",
      expected: expectedCommandId,
      actual: commandId,
    });
  }
  return createCommandReceipt(commandId, value.accepted);
}
