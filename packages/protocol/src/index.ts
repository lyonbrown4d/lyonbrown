import type { ChannelOptions } from "@grpc/grpc-js";

export * from "./auth.js";
export * from "./boundary.js";
export * from "./generated/agent/v1/agent.js";
export * from "./generated/agent/v1/commands.js";
export * from "./generated/agent/v1/snapshot.js";

export const PROTOCOL_VERSION = 1;
export const MAX_GRPC_MESSAGE_BYTES = 16 * 1024 * 1024;

export const GRPC_CHANNEL_OPTIONS = Object.freeze({
  "grpc.max_receive_message_length": MAX_GRPC_MESSAGE_BYTES,
  "grpc.max_send_message_length": MAX_GRPC_MESSAGE_BYTES,
}) satisfies ChannelOptions;

export const METADATA_KEYS = Object.freeze({
  protocolVersion: "protocol_version",
  clientInstanceId: "client_instance_id",
  commandId: "command_id",
  expectedServerInstanceId: "expected_server_instance_id",
  authorization: "authorization",
});
