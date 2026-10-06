import { domainError } from "@lyonbrown/agent-domain/internal/errors";
import {
  asClientInstanceId,
  asCommandId,
  asServerInstanceId,
  type ClientInstanceId,
  type CommandId,
  type ServerInstanceId,
} from "@lyonbrown/agent-domain/internal/identifiers";
import { readDataRecord, requireDataField } from "@lyonbrown/agent-domain/internal/record-boundary";

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

const COMMAND_METADATA_FIELDS = new Set([
  "protocolVersion",
  "clientInstanceId",
  "commandId",
  "expectedServerInstanceId",
]);
const COMMAND_RECEIPT_FIELDS = new Set(["accepted", "protocolVersion", "commandId"]);

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

export function validateCommandMetadata(value: unknown): CommandMetadata {
  const record = readDataRecord(value, "metadata", COMMAND_METADATA_FIELDS);
  const protocolVersion = requireDataField(record, "metadata", "protocolVersion");
  const clientInstanceId = requireDataField(record, "metadata", "clientInstanceId");
  const commandId = requireDataField(record, "metadata", "commandId");
  if (protocolVersion !== PROTOCOL_VERSION) {
    throw domainError("failed_precondition", "Unsupported protocol version", {
      reason: "protocol_version_mismatch",
      expected: PROTOCOL_VERSION,
      ...(typeof protocolVersion === "number" || typeof protocolVersion === "string"
        ? { actual: protocolVersion }
        : {}),
    });
  }
  const input: CommandMetadataInput = {
    clientInstanceId: asClientInstanceId(clientInstanceId),
    commandId: asCommandId(commandId),
    ...(!record.has("expectedServerInstanceId")
      ? {}
      : { expectedServerInstanceId: asServerInstanceId(record.get("expectedServerInstanceId")) }),
  };
  return createCommandMetadata(input);
}

export function createCommandReceipt(commandId: CommandId, accepted: boolean): CommandReceipt {
  return { accepted, protocolVersion: PROTOCOL_VERSION, commandId };
}

export function validateCommandReceipt(
  value: unknown,
  expectedCommandId?: CommandId,
): CommandReceipt {
  const record = readDataRecord(value, "receipt", COMMAND_RECEIPT_FIELDS);
  const protocolVersion = requireDataField(record, "receipt", "protocolVersion");
  const accepted = requireDataField(record, "receipt", "accepted");
  const untrustedCommandId = requireDataField(record, "receipt", "commandId");
  if (protocolVersion !== PROTOCOL_VERSION) {
    throw domainError("failed_precondition", "Unsupported protocol version", {
      reason: "protocol_version_mismatch",
      expected: PROTOCOL_VERSION,
      ...(typeof protocolVersion === "number" || typeof protocolVersion === "string"
        ? { actual: protocolVersion }
        : {}),
    });
  }
  if (typeof accepted !== "boolean") {
    throw domainError("invalid_argument", "Receipt accepted flag must be boolean", {
      field: "accepted",
      reason: "invalid_type",
    });
  }
  const commandId = asCommandId(untrustedCommandId);
  if (expectedCommandId !== undefined && commandId !== expectedCommandId) {
    throw domainError("failed_precondition", "Receipt belongs to a different command", {
      reason: "command_id_mismatch",
      expected: expectedCommandId,
      actual: commandId,
    });
  }
  return createCommandReceipt(commandId, accepted);
}
