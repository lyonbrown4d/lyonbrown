use agent_app::{AgentClientError, ProtocolErrorKind};
use agent_client::{ConnectionState, RemoteState};

use super::TuiState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TuiConnectionState {
  Connecting,
  #[default]
  Connected,
  Reconnecting,
  Unavailable,
  Incompatible,
}

#[derive(Debug, Default)]
pub(super) struct ConnectionProjection {
  state: TuiConnectionState,
  snapshot_stale: bool,
  error: Option<String>,
}

impl TuiState {
  pub fn connection(&self) -> TuiConnectionState {
    self.connection.state
  }

  pub fn connection_error(&self) -> Option<&str> {
    self.connection.error.as_deref()
  }

  pub fn is_snapshot_stale(&self) -> bool {
    self.connection.snapshot_stale
  }

  pub fn can_submit(&self) -> bool {
    self.connection.state == TuiConnectionState::Connected
      && !self.connection.snapshot_stale
      && !self.model_setup_required
  }

  pub fn apply_connection(&mut self, state: TuiConnectionState, error: Option<String>) {
    if state != TuiConnectionState::Connected || self.connection.state != state {
      self.connection.snapshot_stale = true;
    }
    self.connection.state = state;
    self.connection.error = if state == TuiConnectionState::Connected {
      None
    } else {
      error
    };
  }

  pub fn mark_snapshot_fresh(&mut self) {
    if self.connection.state == TuiConnectionState::Connected {
      self.connection.snapshot_stale = false;
    }
  }

  pub fn invalidate_snapshot(&mut self) {
    self.connection.snapshot_stale = true;
  }

  pub(crate) fn record_client_failure(&mut self, error: AgentClientError) {
    let connection = match &error {
      AgentClientError::Unavailable { .. } => Some(TuiConnectionState::Unavailable),
      AgentClientError::Protocol {
        kind: ProtocolErrorKind::IncompatibleVersion,
        ..
      } => Some(TuiConnectionState::Incompatible),
      AgentClientError::Protocol { .. } => Some(TuiConnectionState::Unavailable),
      AgentClientError::Command(_) => None,
    };
    let message = error.to_string();
    if let Some(connection) = connection {
      self.apply_connection(connection, Some(message.clone()));
    }
    self.set_error(message);
  }
}

pub fn apply_remote_state(state: &mut TuiState, remote: &RemoteState) {
  match &remote.connection {
    ConnectionState::Connecting => {
      state.apply_connection(TuiConnectionState::Connecting, None);
    }
    ConnectionState::Connected { server_instance_id } => {
      state.apply_connection(TuiConnectionState::Connected, None);
      match remote.snapshot.as_ref() {
        Some(snapshot) if snapshot.server_instance_id == *server_instance_id => {
          state.sync_snapshot(&snapshot.snapshot);
          state.mark_snapshot_fresh();
        }
        _ => state.invalidate_snapshot(),
      }
    }
    ConnectionState::Reconnecting { error, .. } => {
      state.apply_connection(TuiConnectionState::Reconnecting, Some(error.to_string()));
    }
    ConnectionState::Failed { error, .. } => {
      let connection = match error {
        AgentClientError::Protocol {
          kind: ProtocolErrorKind::IncompatibleVersion,
          ..
        } => TuiConnectionState::Incompatible,
        _ => TuiConnectionState::Unavailable,
      };
      state.apply_connection(connection, Some(error.to_string()));
    }
  }
}
