use agent_app::{
  AgentCommand, ManagedModelView, ModelSetupAction, ModelSetupPhase, ModelSetupSource,
  ModelSetupView,
};
use agent_cli::tui::runner::{
  ModelSetupCommandGate, ModelSetupCompletion, ModelSetupExit, ModelSetupInput,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn pending_setup_exit_waits_for_the_matching_successful_receipt() {
  let required = managed_setup();
  let mut gate = pending_gate(&required);
  let token = gate.pending_token().unwrap();

  assert_eq!(gate.request_exit(), ModelSetupExit::WaitForSetup);
  assert!(gate.is_closing());
  assert_eq!(
    gate.input(
      &required,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    ),
    ModelSetupInput::Ignore
  );
  assert_eq!(
    gate.complete(token.wrapping_add(1), true),
    ModelSetupCompletion::Continue
  );
  assert!(gate.is_closing());
  assert_eq!(gate.complete(token, true), ModelSetupCompletion::Exit);
  assert!(!gate.is_closing());
}

#[test]
fn failed_setup_receipt_clears_closing_and_keeps_the_screen_interactive() {
  let required = managed_setup();
  let mut gate = pending_gate(&required);
  let token = gate.pending_token().unwrap();
  assert_eq!(gate.request_exit(), ModelSetupExit::WaitForSetup);

  assert_eq!(gate.complete(token, false), ModelSetupCompletion::Continue);
  assert!(!gate.is_closing());
  assert!(!gate.is_pending());
  assert!(matches!(
    gate.input(
      &required,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(_)
  ));
}

#[test]
fn setup_exit_without_a_pending_rpc_is_immediate() {
  let mut gate = ModelSetupCommandGate::default();
  assert_eq!(gate.request_exit(), ModelSetupExit::ExitNow);
  assert!(!gate.is_closing());
}

fn pending_gate(setup: &ModelSetupView) -> ModelSetupCommandGate {
  let mut gate = ModelSetupCommandGate::default();
  assert!(matches!(
    gate.input(
      setup,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(AgentCommand::StartModelDownload { .. })
  ));
  gate
}

fn managed_setup() -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase: ModelSetupPhase::Required,
    managed_model: Some(ManagedModelView {
      id: "current-model".into(),
      expected_download_bytes: 2 * 1024 * 1024,
      max_bytes: 3 * 1024 * 1024,
      min_available_memory_bytes: 4 * 1024 * 1024,
      sha256: "abc123".into(),
    }),
    operation_id: Some("operation-old".into()),
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions: vec![ModelSetupAction::Download],
  }
}
