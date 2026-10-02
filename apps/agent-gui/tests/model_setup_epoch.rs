use agent_app::{
  ManagedModelView, ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView,
  RuntimePhase, RuntimeSnapshot,
};
use agent_gui::{
  model_setup::ModelSetupIntent,
  state::{CockpitState, ConnectionState},
};

#[test]
fn a_new_daemon_epoch_releases_pending_model_setup_work() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 7));
  state.apply_runtime_snapshot(&setup_snapshot());
  let epoch_a = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .unwrap();
  assert_eq!(epoch_a.expected_server_instance_id, "daemon-a");

  assert!(state.apply_snapshot_revision("daemon-b", 1));
  state.apply_runtime_snapshot(&setup_snapshot());

  assert!(!state.model_setup_command_pending());
  let epoch_b = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .unwrap();
  assert_eq!(epoch_b.expected_server_instance_id, "daemon-b");
  assert_eq!(epoch_a.expected_server_instance_id, "daemon-a");
}

#[test]
fn a_rejected_revision_cannot_retarget_a_model_setup_dispatch() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 7));
  state.apply_runtime_snapshot(&setup_snapshot());

  assert!(!state.apply_snapshot_revision("daemon-a", 6));
  let dispatch = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .unwrap();

  assert_eq!(dispatch.expected_server_instance_id, "daemon-a");
}

#[test]
fn a_new_daemon_epoch_clears_a_previous_command_error() {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 7));
  state.apply_runtime_snapshot(&setup_snapshot());
  let failed = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .unwrap();
  assert!(state.finish_model_setup_command(failed.token, Some("rpc failed")));
  assert_eq!(state.model_setup_command_error(), Some("rpc failed"));

  assert!(state.apply_snapshot_revision("daemon-b", 1));
  state.apply_runtime_snapshot(&setup_snapshot());

  assert!(!state.model_setup_command_pending());
  assert_eq!(state.model_setup_command_error(), None);
}

fn setup_snapshot() -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(ModelSetupView {
      source: ModelSetupSource::ManagedPreset,
      phase: ModelSetupPhase::Required,
      managed_model: Some(ManagedModelView {
        id: "qwen-test".into(),
        expected_download_bytes: 1,
        max_bytes: 2,
        min_available_memory_bytes: 3,
        sha256: "digest".into(),
      }),
      operation_id: None,
      downloaded_bytes: 0,
      total_bytes: None,
      failure: None,
      allowed_actions: vec![ModelSetupAction::Download],
    }),
    ..RuntimeSnapshot::default()
  }
}
