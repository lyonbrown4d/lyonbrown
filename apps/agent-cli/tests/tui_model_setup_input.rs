use agent_app::{
  AgentCommand, ManagedModelView, ModelSetupAction, ModelSetupPhase, ModelSetupSource,
  ModelSetupView,
};
use agent_cli::tui::runner::{
  ModelSetupCommandGate, ModelSetupCompletion, ModelSetupInput, model_setup_input,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn managed_download_and_retry_use_the_latest_model_id() {
  let mut setup = managed_setup(ModelSetupPhase::Required, vec![ModelSetupAction::Download]);
  assert_eq!(
    input(&setup, true, KeyCode::Enter),
    ModelSetupInput::Dispatch(AgentCommand::StartModelDownload {
      model_id: "current-model".into(),
    })
  );

  setup.phase = ModelSetupPhase::Failed;
  setup.allowed_actions = vec![ModelSetupAction::Retry];
  setup.managed_model.as_mut().unwrap().id = "replacement-model".into();
  for key in [KeyCode::Enter, KeyCode::Char('r'), KeyCode::Char('D')] {
    assert_eq!(
      input(&setup, true, key),
      ModelSetupInput::Dispatch(AgentCommand::StartModelDownload {
        model_id: "replacement-model".into(),
      })
    );
  }
}

#[test]
fn cancellation_uses_the_latest_operation_id() {
  let mut setup = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  setup.operation_id = Some("operation-new".into());

  for key in [KeyCode::Char('c'), KeyCode::Esc] {
    assert_eq!(
      input(&setup, true, key),
      ModelSetupInput::Dispatch(AgentCommand::CancelModelDownload {
        operation_id: "operation-new".into(),
      })
    );
  }
}

#[test]
fn setup_commands_fail_closed_without_permission_identifier_or_fresh_snapshot() {
  let mut setup = managed_setup(ModelSetupPhase::Required, vec![]);
  assert_eq!(input(&setup, true, KeyCode::Enter), ModelSetupInput::Ignore);

  setup.allowed_actions = vec![ModelSetupAction::Download];
  setup.managed_model = None;
  assert_eq!(input(&setup, true, KeyCode::Enter), ModelSetupInput::Ignore);

  setup.managed_model = Some(managed_model("current-model"));
  assert_eq!(
    input(&setup, false, KeyCode::Enter),
    ModelSetupInput::Ignore
  );

  setup.phase = ModelSetupPhase::Downloading;
  setup.allowed_actions = vec![ModelSetupAction::Cancel];
  setup.operation_id = None;
  assert_eq!(input(&setup, true, KeyCode::Esc), ModelSetupInput::Ignore);
  setup.operation_id = Some("operation-new".into());
  assert_eq!(input(&setup, false, KeyCode::Esc), ModelSetupInput::Ignore);
}

#[test]
fn configured_paths_and_non_cancellable_phases_never_dispatch_download_controls() {
  let mut configured = managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download, ModelSetupAction::Retry],
  );
  configured.source = ModelSetupSource::ConfiguredPath;
  for key in [KeyCode::Enter, KeyCode::Char('d'), KeyCode::Char('r')] {
    assert_eq!(input(&configured, true, key), ModelSetupInput::Ignore);
  }

  for phase in [ModelSetupPhase::Installing, ModelSetupPhase::Activating] {
    let setup = managed_setup(phase, vec![ModelSetupAction::Cancel]);
    for key in [KeyCode::Char('c'), KeyCode::Esc] {
      assert_eq!(input(&setup, true, key), ModelSetupInput::Ignore);
    }
  }
}

#[test]
fn quit_and_escape_follow_setup_phase_semantics() {
  for phase in [
    ModelSetupPhase::Required,
    ModelSetupPhase::Cancelled,
    ModelSetupPhase::Failed,
  ] {
    let setup = managed_setup(phase, vec![]);
    assert_eq!(input(&setup, true, KeyCode::Esc), ModelSetupInput::Exit);
    assert_eq!(
      input(&setup, true, KeyCode::Char('q')),
      ModelSetupInput::Exit
    );
  }

  let downloading = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  assert_eq!(
    input(&downloading, true, KeyCode::Char('q')),
    ModelSetupInput::Exit
  );
}

#[test]
fn pending_setup_command_blocks_repeated_keys_until_completion() {
  let setup = managed_setup(ModelSetupPhase::Required, vec![ModelSetupAction::Download]);
  let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
  let mut gate = ModelSetupCommandGate::default();

  assert!(matches!(
    gate.input(&setup, true, key),
    ModelSetupInput::Dispatch(AgentCommand::StartModelDownload { .. })
  ));
  assert!(gate.is_pending());
  let token = gate.pending_token().unwrap();
  assert_eq!(gate.input(&setup, true, key), ModelSetupInput::Ignore);

  assert_eq!(gate.complete(token, true), ModelSetupCompletion::Continue);
  assert!(!gate.is_pending());
  assert!(matches!(
    gate.input(&setup, true, key),
    ModelSetupInput::Dispatch(AgentCommand::StartModelDownload { .. })
  ));
}

#[test]
fn authoritative_snapshot_allows_cancel_and_old_completion_cannot_clear_it() {
  let required = managed_setup(ModelSetupPhase::Required, vec![ModelSetupAction::Download]);
  let mut gate = ModelSetupCommandGate::default();
  assert!(matches!(
    gate.input(
      &required,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(_)
  ));
  let start_token = gate.pending_token().unwrap();

  let downloading = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  gate.observe_snapshot(&downloading);
  assert!(!gate.is_pending());
  assert!(matches!(
    gate.input(
      &downloading,
      true,
      KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(AgentCommand::CancelModelDownload { .. })
  ));
  let cancel_token = gate.pending_token().unwrap();
  assert_ne!(start_token, cancel_token);

  assert_eq!(
    gate.complete(start_token, true),
    ModelSetupCompletion::Continue
  );
  assert!(gate.is_pending());
  assert_eq!(
    gate.complete(cancel_token, true),
    ModelSetupCompletion::Continue
  );
  assert!(!gate.is_pending());
}

#[test]
fn changed_operation_id_is_authoritative_even_before_phase_advances() {
  let failed = managed_setup(ModelSetupPhase::Failed, vec![ModelSetupAction::Retry]);
  let mut gate = ModelSetupCommandGate::default();
  assert!(matches!(
    gate.input(
      &failed,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(AgentCommand::StartModelDownload { .. })
  ));

  let mut restarted = failed;
  restarted.operation_id = Some("operation-new".into());
  restarted.allowed_actions = vec![ModelSetupAction::Cancel];
  gate.observe_snapshot(&restarted);
  assert!(!gate.is_pending());
  assert_eq!(
    gate.input(
      &restarted,
      true,
      KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(AgentCommand::CancelModelDownload {
      operation_id: "operation-new".into()
    })
  );
}

fn input(setup: &ModelSetupView, fresh: bool, code: KeyCode) -> ModelSetupInput {
  model_setup_input(setup, fresh, KeyEvent::new(code, KeyModifiers::NONE))
}

fn managed_setup(phase: ModelSetupPhase, allowed_actions: Vec<ModelSetupAction>) -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase,
    managed_model: Some(managed_model("current-model")),
    operation_id: Some("operation-old".into()),
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions,
  }
}

fn managed_model(id: &str) -> ManagedModelView {
  ManagedModelView {
    id: id.into(),
    expected_download_bytes: 2 * 1024 * 1024,
    max_bytes: 3 * 1024 * 1024,
    min_available_memory_bytes: 4 * 1024 * 1024,
    sha256: "abc123".into(),
  }
}
