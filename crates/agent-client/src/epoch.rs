use agent_app::{AgentClientError, AvailabilityKind};

use crate::ConnectionState;

pub(crate) fn connected_server(state: ConnectionState) -> Result<String, AgentClientError> {
  match state {
    ConnectionState::Connected { server_instance_id } => Ok(server_instance_id),
    ConnectionState::Failed { error, .. } => Err(error),
    _ => Err(disconnected_error()),
  }
}

pub(crate) fn ensure_expected_server(
  state: ConnectionState,
  expected_server_instance_id: &str,
) -> Result<(), AgentClientError> {
  let actual = connected_server(state)?;
  if actual == expected_server_instance_id {
    return Ok(());
  }
  Err(AgentClientError::unavailable(
    AvailabilityKind::StaleEpoch,
    format!(
      "agent daemon epoch changed (expected {expected_server_instance_id}, connected {actual})"
    ),
  ))
}

fn disconnected_error() -> AgentClientError {
  AgentClientError::unavailable(
    AvailabilityKind::Disconnected,
    "agent daemon is not connected",
  )
}
