use agent_client::{
  AgentClientError, AvailabilityKind, ConnectionState as RemoteConnectionState, ProtocolErrorKind,
  RemoteState, VersionedRemoteSnapshot,
};
use agent_gui::{
  client_state::{RemoteStatePump, apply_remote_state},
  state::{CockpitState, ConnectionState},
};
use tokio::sync::watch;

fn snapshot(server_instance_id: &str, revision: u64) -> VersionedRemoteSnapshot {
  VersionedRemoteSnapshot {
    server_instance_id: server_instance_id.into(),
    revision,
    snapshot: Default::default(),
  }
}

fn connected(server_instance_id: &str, revision: u64) -> RemoteState {
  RemoteState {
    connection: RemoteConnectionState::Connected {
      server_instance_id: server_instance_id.into(),
    },
    snapshot: Some(snapshot(server_instance_id, revision)),
  }
}

#[test]
fn connected_snapshot_can_restart_revision_for_a_new_daemon_epoch() {
  let mut state = CockpitState::default();
  apply_remote_state(&mut state, &connected("daemon-a", 42));

  apply_remote_state(&mut state, &connected("daemon-b", 1));

  assert_eq!(state.connection(), ConnectionState::Connected);
  assert_eq!(state.server_instance_id(), Some("daemon-b"));
  assert_eq!(state.revision(), Some(1));
  assert!(!state.is_stale());
  assert!(state.can_submit());
}

#[test]
fn same_revision_connected_snapshot_confirms_reconnect_resync() {
  let mut state = CockpitState::default();
  apply_remote_state(&mut state, &connected("daemon-a", 7));
  apply_remote_state(
    &mut state,
    &RemoteState {
      connection: RemoteConnectionState::Reconnecting {
        server_instance_id: Some("daemon-a".into()),
        attempt: 1,
        error: AgentClientError::unavailable(AvailabilityKind::Disconnected, "stream closed"),
      },
      snapshot: Some(snapshot("daemon-a", 7)),
    },
  );
  assert!(state.is_stale());

  apply_remote_state(&mut state, &connected("daemon-a", 7));

  assert!(!state.is_stale());
  assert!(state.can_submit());
}

#[test]
fn reconnecting_keeps_the_cached_revision_stale_and_reports_error() {
  let mut state = CockpitState::default();
  let remote = RemoteState {
    connection: RemoteConnectionState::Reconnecting {
      server_instance_id: Some("daemon-a".into()),
      attempt: 2,
      error: AgentClientError::unavailable(
        AvailabilityKind::Disconnected,
        "runtime stream disconnected",
      ),
    },
    snapshot: Some(snapshot("daemon-a", 9)),
  };

  apply_remote_state(&mut state, &remote);

  assert_eq!(state.connection(), ConnectionState::Reconnecting);
  assert_eq!(state.server_instance_id(), Some("daemon-a"));
  assert_eq!(state.revision(), Some(9));
  assert!(state.is_stale());
  assert!(!state.can_submit());
  assert!(
    state
      .last_connection_error()
      .is_some_and(|error| error.contains("runtime stream disconnected"))
  );
}

#[test]
fn incompatible_protocol_failure_maps_to_incompatible() {
  let mut state = CockpitState::default();
  let remote = RemoteState {
    connection: RemoteConnectionState::Failed {
      server_instance_id: Some("daemon-a".into()),
      error: AgentClientError::protocol(
        ProtocolErrorKind::IncompatibleVersion,
        "server protocol is newer",
      ),
    },
    snapshot: None,
  };

  apply_remote_state(&mut state, &remote);

  assert_eq!(state.connection(), ConnectionState::Incompatible);
  assert!(
    state
      .last_connection_error()
      .is_some_and(|error| error.contains("server protocol is newer"))
  );
  assert!(!state.can_submit());
}

#[test]
fn other_failures_map_to_unavailable_and_keep_the_cached_snapshot() {
  let mut state = CockpitState::default();
  let remote = RemoteState {
    connection: RemoteConnectionState::Failed {
      server_instance_id: Some("daemon-a".into()),
      error: AgentClientError::unavailable(AvailabilityKind::Timeout, "connect timeout"),
    },
    snapshot: Some(snapshot("daemon-a", 11)),
  };

  apply_remote_state(&mut state, &remote);

  assert_eq!(state.connection(), ConnectionState::Unavailable);
  assert_eq!(state.revision(), Some(11));
  assert!(state.is_stale());
  assert!(
    state
      .last_connection_error()
      .is_some_and(|error| error.contains("connect timeout"))
  );
}

#[test]
fn connected_without_a_matching_snapshot_never_enables_submission() {
  let mut state = CockpitState::default();
  apply_remote_state(&mut state, &connected("daemon-a", 5));
  let mismatched = RemoteState {
    connection: RemoteConnectionState::Connected {
      server_instance_id: "daemon-b".into(),
    },
    snapshot: Some(snapshot("daemon-a", 5)),
  };

  apply_remote_state(&mut state, &mismatched);

  assert_eq!(state.connection(), ConnectionState::Connected);
  assert_eq!(state.server_instance_id(), Some("daemon-a"));
  assert_eq!(state.revision(), Some(5));
  assert!(state.is_stale());
  assert!(!state.can_submit());
}

#[test]
fn pump_applies_the_current_remote_state_before_waiting() {
  let (_sender, receiver) = watch::channel(connected("daemon-a", 12));
  let mut pump = RemoteStatePump::new(receiver);
  let mut state = CockpitState::default();

  pump.apply_current(&mut state);

  assert_eq!(state.server_instance_id(), Some("daemon-a"));
  assert_eq!(state.revision(), Some(12));
  assert!(state.can_submit());
}

#[tokio::test]
async fn pump_yields_each_changed_atomic_remote_state() {
  let (sender, receiver) = watch::channel(connected("daemon-a", 3));
  let mut pump = RemoteStatePump::new(receiver);
  let mut state = CockpitState::default();
  pump.apply_current(&mut state);

  sender.send_replace(connected("daemon-b", 1));
  let remote = pump.next().await.expect("remote state channel closed");
  apply_remote_state(&mut state, &remote);

  assert_eq!(state.server_instance_id(), Some("daemon-b"));
  assert_eq!(state.revision(), Some(1));
}
