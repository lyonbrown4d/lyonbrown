import type { EpochBinding } from "@lyonbrown/local-control-plane";
import { PROTOCOL_VERSION, type HandshakeResponse } from "@lyonbrown/protocol";

import { clientError } from "./errors.js";
import type { HandshakeDto } from "./types.js";
import { protocolId } from "./validation.js";

export function mapHandshake(response: HandshakeResponse, binding: EpochBinding): HandshakeDto {
  if (response.protocolVersion !== PROTOCOL_VERSION) {
    throw clientError(
      "protocol",
      "INCOMPATIBLE_VERSION",
      "server protocol version is incompatible",
    );
  }
  if (response.serverInstanceId !== binding.serverInstanceId) {
    throw clientError(
      "protocol",
      "SERVER_INSTANCE_MISMATCH",
      "server instance does not match discovery",
      {
        expectedServerInstanceId: binding.serverInstanceId,
        actualServerInstanceId: response.serverInstanceId,
      },
    );
  }
  if (response.scopeFingerprint !== binding.scopeId) {
    throw clientError("protocol", "SCOPE_MISMATCH", "server scope does not match discovery");
  }
  return {
    protocolVersion: response.protocolVersion,
    serverVersion: protocolId("handshake.server_version", response.serverVersion),
    serverInstanceId: response.serverInstanceId,
    capabilities: response.capabilities.map((capability) =>
      protocolId("handshake.capability", capability),
    ),
    scopeId: response.scopeFingerprint,
  };
}
