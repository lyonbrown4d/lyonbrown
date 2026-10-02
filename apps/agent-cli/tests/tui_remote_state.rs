use agent_app::{AgentClientError, ProtocolErrorKind, RuntimePhase, RuntimeSnapshot};
use agent_cli::tui::{TuiConnectionState, TuiState, apply_remote_state};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};

#[test]
fn connected_remote_snapshot_becomes_fresh_atomically() {
  let mut state = TuiState::default();
  let remote = RemoteState {
    connection: ConnectionState::Connected {
      server_instance_id: "server-1".into(),
    },
    snapshot: Some(VersionedRemoteSnapshot {
      server_instance_id: "server-1".into(),
      revision: 7,
      snapshot: RuntimeSnapshot {
        phase: RuntimePhase::Ready,
        ..RuntimeSnapshot::default()
      },
    }),
  };

  apply_remote_state(&mut state, &remote);

  assert_eq!(state.connection(), TuiConnectionState::Connected);
  assert!(!state.is_snapshot_stale());
  assert!(state.can_submit());
}

#[test]
fn reconnecting_remote_state_keeps_the_projected_snapshot_stale() {
  let mut state = TuiState::default();
  state.mark_snapshot_fresh();
  let remote = RemoteState {
    connection: ConnectionState::Reconnecting {
      server_instance_id: Some("server-1".into()),
      attempt: 2,
      error: AgentClientError::unavailable(
        agent_app::AvailabilityKind::Disconnected,
        "daemon unavailable",
      ),
    },
    snapshot: None,
  };

  apply_remote_state(&mut state, &remote);

  assert_eq!(state.connection(), TuiConnectionState::Reconnecting);
  assert!(state.is_snapshot_stale());
  assert!(!state.can_submit());
  assert!(
    state
      .connection_error()
      .unwrap()
      .contains("daemon unavailable")
  );
}

#[test]
fn incompatible_protocol_is_distinct_from_unavailable() {
  let mut state = TuiState::default();
  let remote = RemoteState {
    connection: ConnectionState::Failed {
      server_instance_id: None,
      error: AgentClientError::protocol(ProtocolErrorKind::IncompatibleVersion, "upgrade required"),
    },
    snapshot: None,
  };

  apply_remote_state(&mut state, &remote);

  assert_eq!(state.connection(), TuiConnectionState::Incompatible);
  assert!(state.is_snapshot_stale());
}
