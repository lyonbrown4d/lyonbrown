use agent_gui::state::{CockpitState, ConnectionState, RuntimeDetailsPlacement};

#[test]
fn connection_starts_disconnected_without_a_snapshot() {
  let state = CockpitState::default();

  assert_eq!(state.connection(), ConnectionState::Disconnected);
  assert_eq!(state.server_instance_id(), None);
  assert_eq!(state.revision(), None);
  assert!(!state.is_stale());
  assert!(!state.can_submit());
  assert_eq!(state.last_connection_error(), None);
}

#[test]
fn connection_transitions_control_submission_and_error_state() {
  let mut state = CockpitState::default();

  state.apply_connection(ConnectionState::Connecting, None);
  assert_eq!(state.connection(), ConnectionState::Connecting);
  assert!(!state.can_submit());

  state.apply_connection(ConnectionState::Connected, None);
  assert_eq!(state.connection(), ConnectionState::Connected);
  assert!(!state.can_submit());

  assert!(state.apply_snapshot_revision("daemon-a", 1));
  assert!(state.can_submit());
  assert_eq!(state.last_connection_error(), None);

  state.apply_connection(
    ConnectionState::Unavailable,
    Some("agentd is unavailable".into()),
  );
  assert_eq!(state.connection(), ConnectionState::Unavailable);
  assert!(!state.can_submit());
  assert_eq!(state.last_connection_error(), Some("agentd is unavailable"));

  state.apply_connection(
    ConnectionState::Incompatible,
    Some("protocol version mismatch".into()),
  );
  assert_eq!(state.connection(), ConnectionState::Incompatible);
  assert!(!state.can_submit());
  assert_eq!(
    state.last_connection_error(),
    Some("protocol version mismatch")
  );
}

#[test]
fn revisions_only_advance_within_one_daemon_instance() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);

  assert!(state.apply_snapshot_revision("daemon-a", 7));
  assert_eq!(state.server_instance_id(), Some("daemon-a"));
  assert_eq!(state.revision(), Some(7));
  assert!(!state.is_stale());
  assert!(!state.apply_snapshot_revision("daemon-a", 7));
  assert!(!state.apply_snapshot_revision("daemon-a", 6));
  assert_eq!(state.revision(), Some(7));
}

#[test]
fn reconnect_requires_a_snapshot_before_submission_resumes() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 7));

  state.apply_connection(
    ConnectionState::Reconnecting,
    Some("connection lost".into()),
  );
  assert_eq!(state.revision(), Some(7));
  assert!(state.is_stale());
  assert!(!state.can_submit());

  state.apply_connection(ConnectionState::Connected, None);
  assert_eq!(state.revision(), Some(7));
  assert!(state.is_stale());
  assert!(!state.can_submit());
}

#[test]
fn same_revision_snapshot_confirms_a_resynchronized_connection() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 7));
  state.apply_connection(ConnectionState::Reconnecting, None);
  state.apply_connection(ConnectionState::Connected, None);

  assert!(state.apply_snapshot_revision("daemon-a", 7));
  assert!(!state.is_stale());
  assert!(state.can_submit());
  assert!(!state.apply_snapshot_revision("daemon-a", 7));
}

#[test]
fn a_new_daemon_instance_can_restart_at_a_lower_revision() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 42));
  state.apply_connection(ConnectionState::Reconnecting, None);
  state.apply_connection(ConnectionState::Connected, None);

  assert!(state.apply_snapshot_revision("daemon-b", 1));
  assert_eq!(state.server_instance_id(), Some("daemon-b"));
  assert_eq!(state.revision(), Some(1));
  assert!(!state.is_stale());
  assert!(state.can_submit());
}

#[test]
fn snapshot_received_before_connected_needs_confirmation_after_handshake() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connecting, None);

  assert!(state.apply_snapshot_revision("daemon-a", 3));
  assert!(state.is_stale());

  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.is_stale());
  assert!(!state.can_submit());

  assert!(state.apply_snapshot_revision("daemon-a", 3));
  assert!(state.can_submit());
}

#[test]
fn runtime_details_start_collapsed() {
  let state = CockpitState::default();

  assert!(!state.runtime_details_expanded());
  assert_eq!(state.runtime_details_control_label(), "Show details");
}

#[test]
fn runtime_details_control_toggles_preview_visibility() {
  let mut state = CockpitState::default();

  state.toggle_runtime_details();
  assert!(state.runtime_details_expanded());
  assert_eq!(state.runtime_details_control_label(), "Hide details");

  state.toggle_runtime_details();
  assert!(!state.runtime_details_expanded());
  assert_eq!(state.runtime_details_control_label(), "Show details");
}

#[test]
fn expanded_details_render_in_the_visible_region_for_each_layout() {
  assert_eq!(
    RuntimeDetailsPlacement::for_layout(false, false),
    RuntimeDetailsPlacement::Hidden
  );
  assert_eq!(
    RuntimeDetailsPlacement::for_layout(true, true),
    RuntimeDetailsPlacement::Conversation
  );
  assert_eq!(
    RuntimeDetailsPlacement::for_layout(false, true),
    RuntimeDetailsPlacement::RuntimeRail
  );
}
