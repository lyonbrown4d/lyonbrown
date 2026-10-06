import type { ChannelOptions } from "@grpc/grpc-js";

export * from "@lyonbrown/protocol/internal/auth";
export * from "@lyonbrown/protocol/internal/boundary";
export * from "@lyonbrown/protocol/generated/agent/v1/agent";
export * from "@lyonbrown/protocol/generated/agent/v1/commands";
export * from "@lyonbrown/protocol/generated/agent/v1/snapshot";

export const PROTOCOL_VERSION = 1;
export const MAX_GRPC_MESSAGE_BYTES = 16 * 1024 * 1024;

export const GRPC_CHANNEL_OPTIONS = Object.freeze({
  "grpc.enable_http_proxy": 0,
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
