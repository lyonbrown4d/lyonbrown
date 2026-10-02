use agent_client::{
  AgentClientError, ConnectionState as RemoteConnectionState, ProtocolErrorKind, RemoteState,
  VersionedRemoteSnapshot,
};
use tokio::sync::watch;

use crate::state::{CockpitState, ConnectionState};

/// Pulls atomic remote projections without owning or polling the transport.
pub struct RemoteStatePump {
  receiver: watch::Receiver<RemoteState>,
}

impl RemoteStatePump {
  pub fn new(receiver: watch::Receiver<RemoteState>) -> Self {
    Self { receiver }
  }

  pub fn apply_current(&mut self, state: &mut CockpitState) {
    let remote = self.receiver.borrow_and_update().clone();
    apply_remote_state(state, &remote);
  }

  pub async fn next(&mut self) -> Option<RemoteState> {
    self.receiver.changed().await.ok()?;
    Some(self.receiver.borrow_and_update().clone())
  }
}

/// Applies one authoritative remote projection without running transport work.
pub fn apply_remote_state(state: &mut CockpitState, remote: &RemoteState) {
  match &remote.connection {
    RemoteConnectionState::Connecting => {
      apply_cached_snapshot(state, remote.snapshot.as_ref());
      state.apply_connection(ConnectionState::Connecting, None);
    }
    RemoteConnectionState::Connected { server_instance_id } => {
      state.apply_connection(ConnectionState::Connected, None);
      let snapshot = remote
        .snapshot
        .as_ref()
        .filter(|snapshot| snapshot.server_instance_id == *server_instance_id);
      if let Some(snapshot) = snapshot {
        state.apply_snapshot_revision(&snapshot.server_instance_id, snapshot.revision);
        state.apply_runtime_phase(snapshot.snapshot.phase);
      } else {
        state.mark_snapshot_stale();
      }
    }
    RemoteConnectionState::Reconnecting { error, .. } => {
      apply_cached_snapshot(state, remote.snapshot.as_ref());
      state.apply_connection(ConnectionState::Reconnecting, Some(error.to_string()));
    }
    RemoteConnectionState::Failed { error, .. } => {
      apply_cached_snapshot(state, remote.snapshot.as_ref());
      let connection = if is_incompatible(error) {
        ConnectionState::Incompatible
      } else {
        ConnectionState::Unavailable
      };
      state.apply_connection(connection, Some(error.to_string()));
    }
  }
}

fn apply_cached_snapshot(state: &mut CockpitState, snapshot: Option<&VersionedRemoteSnapshot>) {
  if let Some(snapshot) = snapshot {
    state.apply_snapshot_revision(&snapshot.server_instance_id, snapshot.revision);
    state.apply_runtime_phase(snapshot.snapshot.phase);
  }
}

fn is_incompatible(error: &AgentClientError) -> bool {
  matches!(
    error,
    AgentClientError::Protocol {
      kind: ProtocolErrorKind::IncompatibleVersion,
      ..
    }
  )
}
