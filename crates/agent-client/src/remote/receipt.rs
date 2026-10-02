use agent_app::{AgentClientError, ProtocolErrorKind};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::CommandResponse;

pub(super) fn validate(
  response: &CommandResponse,
  expected_command_id: &str,
) -> Result<(), AgentClientError> {
  if response.protocol_version != PROTOCOL_VERSION {
    return Err(AgentClientError::protocol(
      ProtocolErrorKind::InvalidResponse,
      format!(
        "command receipt protocol version mismatch (expected {PROTOCOL_VERSION}, received {})",
        response.protocol_version
      ),
    ));
  }
  if response.command_id != expected_command_id {
    return Err(AgentClientError::protocol(
      ProtocolErrorKind::InvalidResponse,
      format!(
        "command receipt command id mismatch (expected {expected_command_id}, received {})",
        response.command_id
      ),
    ));
  }
  if !response.accepted {
    return Err(AgentClientError::protocol(
      ProtocolErrorKind::InvalidResponse,
      format!("command receipt was not accepted for command {expected_command_id}"),
    ));
  }
  Ok(())
}
