use agent_app::{AgentClientError, ProtocolErrorKind};
use agent_client::{ConnectionState, RemoteState};

use super::{TuiState, locale::Messages, status_line_safe};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TuiConnectionState {
  Connecting,
  #[default]
  Connected,
  Reconnecting,
  Unavailable,
  Incompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProjection {
  Accepted,
  Unchanged,
  Rejected,
}

pub(super) fn connection_label(state: TuiConnectionState, messages: Messages) -> &'static str {
  match state {
    TuiConnectionState::Connecting => messages.connecting,
    TuiConnectionState::Connected => messages.connected,
    TuiConnectionState::Reconnecting => messages.reconnecting,
    TuiConnectionState::Unavailable => messages.unavailable,
    TuiConnectionState::Incompatible => messages.incompatible,
  }
}

#[derive(Debug, Default)]
pub(super) struct ConnectionProjection {
  state: TuiConnectionState,
  snapshot_stale: bool,
  accepted_server_instance_id: Option<String>,
  accepted_revision: Option<u64>,
  remote_runtime_synced: bool,
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

  pub fn accepted_server_instance_id(&self) -> Option<&str> {
    self.connection.accepted_server_instance_id.as_deref()
  }

  pub(crate) const fn has_synced_remote_runtime(&self) -> bool {
    self.connection.remote_runtime_synced
  }

  pub(crate) fn mark_remote_runtime_synced(&mut self) {
    self.connection.remote_runtime_synced = true;
  }

  pub fn can_submit(&self) -> bool {
    self.connection.state == TuiConnectionState::Connected
      && !self.connection.snapshot_stale
      && self.connection.accepted_server_instance_id.is_some()
      && !self.is_model_setup_required()
  }

  pub fn can_issue_setup_commands(&self) -> bool {
    self.connection.state == TuiConnectionState::Connected
      && !self.connection.snapshot_stale
      && self.connection.accepted_server_instance_id.is_some()
  }

  pub fn apply_connection(&mut self, state: TuiConnectionState, error: Option<String>) {
    if state != TuiConnectionState::Connected || self.connection.state != state {
      self.connection.snapshot_stale = true;
    }
    self.connection.state = state;
    self.connection.error = if state == TuiConnectionState::Connected {
      None
    } else {
      error.map(|message| status_line_safe(&message))
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

  fn accept_remote_snapshot(
    &mut self,
    server_instance_id: &str,
    revision: u64,
    snapshot: &agent_app::RuntimeSnapshot,
  ) -> RemoteProjection {
    let epoch_changed = self
      .connection
      .accepted_server_instance_id
      .as_deref()
      .is_some_and(|current| current != server_instance_id);
    let revision_changed = epoch_changed || self.connection.accepted_revision != Some(revision);
    self.connection.accepted_server_instance_id = Some(server_instance_id.to_owned());
    if epoch_changed {
      self.reset_epoch_transients();
      self.clear_command_error();
    }
    if !revision_changed {
      self.mark_snapshot_fresh();
      return RemoteProjection::Unchanged;
    }
    self.connection.accepted_revision = Some(revision);
    self.sync_snapshot(snapshot);
    self.mark_snapshot_fresh();
    RemoteProjection::Accepted
  }

  #[doc(hidden)]
  pub fn record_client_failure(&mut self, error: AgentClientError) {
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
    self.set_command_error(status_line_safe(&message));
    if let Some(connection) = connection {
      self.apply_connection(connection, Some(message.clone()));
    }
    self.set_error(message);
  }
}

pub fn apply_remote_state(state: &mut TuiState, remote: &RemoteState) -> RemoteProjection {
  match &remote.connection {
    ConnectionState::Connecting => {
      state.apply_connection(TuiConnectionState::Connecting, None);
      RemoteProjection::Rejected
    }
    ConnectionState::Connected { server_instance_id } => {
      state.apply_connection(TuiConnectionState::Connected, None);
      match remote.snapshot.as_ref() {
        Some(snapshot) if snapshot.server_instance_id == *server_instance_id => {
          state.accept_remote_snapshot(server_instance_id, snapshot.revision, &snapshot.snapshot)
        }
        _ => {
          state.invalidate_snapshot();
          RemoteProjection::Rejected
        }
      }
    }
    ConnectionState::Reconnecting { error, .. } => {
      state.apply_connection(TuiConnectionState::Reconnecting, Some(error.to_string()));
      RemoteProjection::Rejected
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
      RemoteProjection::Rejected
    }
  }
}
