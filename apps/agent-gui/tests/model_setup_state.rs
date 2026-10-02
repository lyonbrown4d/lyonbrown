use agent_app::{
  AgentCommand, ManagedModelView, ModelSetupAction, ModelSetupFailureView, ModelSetupPhase,
  ModelSetupSource, ModelSetupView, RuntimePhase, RuntimeSnapshot,
};
use agent_gui::{
  model_setup::{ModelSetupIntent, UiLocale, present_model_setup},
  state::{CockpitState, ConnectionState},
};

#[test]
fn authoritative_snapshot_is_retained_without_reducing_setup_to_a_boolean() {
  let mut state = fresh_state();
  let setup = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  state.apply_runtime_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(setup.clone()),
    ..RuntimeSnapshot::default()
  });

  assert_eq!(state.runtime_phase(), RuntimePhase::ModelSetupRequired);
  assert_eq!(state.model_setup(), Some(&setup));
  assert!(!state.can_submit());
}

#[test]
fn download_retry_and_cancel_use_ids_from_the_current_allowed_view() {
  let mut state = fresh_state();
  state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download],
  )));
  let start = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .expect("download should be allowed");
  assert_eq!(
    start.command,
    AgentCommand::StartModelDownload {
      model_id: "qwen-test".into()
    }
  );

  let mut retry_state = fresh_state();
  retry_state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Failed,
    vec![ModelSetupAction::Retry],
  )));
  assert!(matches!(
    retry_state
      .begin_model_setup_command(ModelSetupIntent::Retry)
      .unwrap()
      .command,
    AgentCommand::StartModelDownload { model_id } if model_id == "qwen-test"
  ));

  let mut cancel_state = fresh_state();
  cancel_state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Downloading,
    vec![ModelSetupAction::Cancel],
  )));
  assert_eq!(
    cancel_state
      .begin_model_setup_command(ModelSetupIntent::Cancel)
      .unwrap()
      .command,
    AgentCommand::CancelModelDownload {
      operation_id: "operation-7".into()
    }
  );
}

#[test]
fn malformed_or_unsafe_views_fail_closed() {
  let cases = [
    (
      ModelSetupSource::ConfiguredPath,
      ModelSetupPhase::Required,
      vec![ModelSetupAction::Download],
      ModelSetupIntent::Start,
    ),
    (
      ModelSetupSource::ManagedPreset,
      ModelSetupPhase::Installing,
      vec![ModelSetupAction::Cancel],
      ModelSetupIntent::Cancel,
    ),
    (
      ModelSetupSource::ManagedPreset,
      ModelSetupPhase::Activating,
      vec![ModelSetupAction::Cancel],
      ModelSetupIntent::Cancel,
    ),
  ];
  for (source, phase, actions, intent) in cases {
    let mut state = fresh_state();
    let mut setup = managed_setup(phase, actions);
    setup.source = source;
    state.apply_runtime_snapshot(&snapshot(setup));
    assert!(state.begin_model_setup_command(intent).is_none());
  }

  let mut stale = fresh_state();
  stale.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download],
  )));
  stale.apply_connection(ConnectionState::Reconnecting, Some("lost".into()));
  assert!(
    stale
      .begin_model_setup_command(ModelSetupIntent::Start)
      .is_none()
  );
}

#[test]
fn pending_tokens_prevent_duplicates_and_old_completions_cannot_clear_new_work() {
  let mut state = fresh_state();
  state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download],
  )));
  let start = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .unwrap();
  assert!(state.model_setup_command_pending());
  assert!(
    state
      .begin_model_setup_command(ModelSetupIntent::Start)
      .is_none()
  );

  state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Downloading,
    vec![ModelSetupAction::Cancel],
  )));
  assert!(!state.model_setup_command_pending());
  let cancel = state
    .begin_model_setup_command(ModelSetupIntent::Cancel)
    .unwrap();
  assert_ne!(start.token, cancel.token);
  let mut progress = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  progress.downloaded_bytes = 1;
  state.apply_runtime_snapshot(&snapshot(progress));
  assert!(state.model_setup_command_pending());

  assert!(!state.finish_model_setup_command(start.token, Some("late failure")));
  assert!(state.model_setup_command_pending());
  assert_eq!(state.model_setup_command_error(), None);
  assert!(state.finish_model_setup_command(cancel.token, Some("rpc\r\nfailed")));
  assert_eq!(state.model_setup_command_error(), Some("rpc  failed"));
}

#[test]
fn successful_rpc_stays_pending_until_the_authoritative_snapshot_advances() {
  let mut state = fresh_state();
  state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download],
  )));
  let ticket = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .unwrap();

  assert!(state.finish_model_setup_command(ticket.token, None));
  assert!(state.model_setup_command_pending());
  assert!(
    state
      .begin_model_setup_command(ModelSetupIntent::Start)
      .is_none()
  );

  state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Downloading,
    vec![ModelSetupAction::Cancel],
  )));
  assert!(!state.model_setup_command_pending());
}

#[test]
fn command_error_persists_until_a_new_command_or_related_snapshot_change() {
  let setup = managed_setup(ModelSetupPhase::Required, vec![ModelSetupAction::Download]);
  let mut state = fresh_state();
  state.apply_runtime_snapshot(&snapshot(setup.clone()));
  let ticket = state
    .begin_model_setup_command(ModelSetupIntent::Start)
    .unwrap();
  state.finish_model_setup_command(ticket.token, Some(&"x".repeat(700)));
  assert_eq!(
    state.model_setup_command_error().unwrap().chars().count(),
    512
  );

  state.apply_runtime_snapshot(&snapshot(setup));
  assert!(state.model_setup_command_error().is_some());
  assert!(
    state
      .begin_model_setup_command(ModelSetupIntent::Start)
      .is_some()
  );
  assert_eq!(state.model_setup_command_error(), None);

  state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Downloading,
    vec![ModelSetupAction::Cancel],
  )));
  assert_eq!(state.model_setup_command_error(), None);
}

#[test]
fn bilingual_presentation_covers_progress_failure_and_technical_values() {
  let mut setup = managed_setup(ModelSetupPhase::Failed, vec![ModelSetupAction::Retry]);
  setup.downloaded_bytes = 512;
  setup.total_bytes = Some(1024);
  setup.failure = Some(ModelSetupFailureView {
    code: "checksum_mismatch".into(),
    message: "digest mismatch".into(),
    retryable: true,
  });

  let en = present_model_setup(&setup, UiLocale::EnUs, true, false, None);
  let zh = present_model_setup(&setup, UiLocale::ZhCn, true, false, None);
  assert_eq!(en.title, "MODEL SETUP");
  assert_eq!(zh.title, "模型配置");
  assert!(en.phase.contains("Failed"));
  assert!(zh.phase.contains("失败"));
  assert_eq!(en.progress.as_deref(), Some("50% · 512 B / 1.00 KiB"));
  assert_eq!(zh.progress.as_deref(), Some("50% · 512 B / 1.00 KiB"));
  assert!(en.failure.as_deref().unwrap().contains("checksum_mismatch"));
  assert!(zh.failure.as_deref().unwrap().contains("checksum_mismatch"));
  assert_eq!(en.primary.unwrap().id, "model-setup-retry");
  assert_eq!(zh.primary.unwrap().label, "重试下载");
}

#[test]
fn unknown_progress_and_configured_path_are_explicit_and_actionless() {
  let mut setup = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  setup.downloaded_bytes = 1536;
  setup.total_bytes = None;
  let en = present_model_setup(&setup, UiLocale::EnUs, false, false, None);
  assert_eq!(
    en.progress.as_deref(),
    Some("Unknown total · 1.50 KiB downloaded")
  );
  assert!(!en.cancel.unwrap().enabled);

  setup.source = ModelSetupSource::ConfiguredPath;
  setup.managed_model = None;
  setup.phase = ModelSetupPhase::Required;
  setup.allowed_actions = vec![ModelSetupAction::Download];
  let configured = present_model_setup(&setup, UiLocale::EnUs, true, false, None);
  assert!(configured.source.contains("llamacpp.model_path"));
  assert!(configured.primary.is_none());
}

fn fresh_state() -> CockpitState {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  assert!(state.apply_snapshot_revision("daemon-a", 1));
  state
}

fn snapshot(model_setup: ModelSetupView) -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(model_setup),
    ..RuntimeSnapshot::default()
  }
}

fn managed_setup(phase: ModelSetupPhase, allowed_actions: Vec<ModelSetupAction>) -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase,
    managed_model: Some(ManagedModelView {
      id: "qwen-test".into(),
      expected_download_bytes: 2 * 1024 * 1024,
      max_bytes: 3 * 1024 * 1024,
      min_available_memory_bytes: 4 * 1024 * 1024,
      sha256: "abc123".into(),
    }),
    operation_id: Some("operation-7".into()),
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions,
  }
}
