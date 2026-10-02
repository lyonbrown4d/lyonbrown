use agent_app::{
  AgentClientError, ManagedModelView, ModelSetupAction, ModelSetupPhase, ModelSetupSource,
  ModelSetupView, RuntimeCommandError, RuntimePhase, RuntimeSnapshot,
};
use agent_cli::tui::TuiState;

#[test]
fn command_error_survives_progress_only_snapshot_updates() {
  let mut snapshot = setup_snapshot(ModelSetupPhase::Downloading);
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot);
  record_download_error(&mut state);

  let setup = snapshot.model_setup.as_mut().unwrap();
  setup.downloaded_bytes = 1024;
  setup.total_bytes = Some(4096);
  state.sync_snapshot(&snapshot);

  assert!(state.command_error().is_some());
}

#[test]
fn command_error_clears_when_setup_phase_advances() {
  let mut snapshot = setup_snapshot(ModelSetupPhase::Downloading);
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot);
  record_download_error(&mut state);

  snapshot.model_setup.as_mut().unwrap().phase = ModelSetupPhase::Verifying;
  state.sync_snapshot(&snapshot);

  assert_eq!(state.command_error(), None);
}

fn record_download_error(state: &mut TuiState) {
  state.record_client_failure(AgentClientError::Command(
    RuntimeCommandError::ModelDownloadUnavailable,
  ));
}

fn setup_snapshot(phase: ModelSetupPhase) -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(ModelSetupView {
      source: ModelSetupSource::ManagedPreset,
      phase,
      managed_model: Some(ManagedModelView {
        id: "qwen-test".into(),
        expected_download_bytes: 2048,
        max_bytes: 4096,
        min_available_memory_bytes: 8192,
        sha256: "abc123".into(),
      }),
      operation_id: Some("operation-7".into()),
      downloaded_bytes: 0,
      total_bytes: None,
      failure: None,
      allowed_actions: vec![ModelSetupAction::Cancel],
    }),
    ..RuntimeSnapshot::default()
  }
}
