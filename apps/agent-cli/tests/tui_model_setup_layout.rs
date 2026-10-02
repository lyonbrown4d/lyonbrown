use agent_app::{
  ManagedModelView, ModelSetupAction, ModelSetupFailureView, ModelSetupPhase, ModelSetupSource,
  ModelSetupView, RuntimePhase, RuntimeSnapshot,
};
use agent_cli::tui::{Locale, RenderContext, TuiState, apply_remote_state, render_with_context};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn compact_setup_layout_prioritizes_phase_actions_and_failure() {
  let mut required_setup = setup(ModelSetupPhase::Required);
  required_setup.allowed_actions = vec![ModelSetupAction::Download];
  let required = screen(&state_with(required_setup), Locale::EnUs, 30, 8);
  assert!(required.contains("phase:"), "{required}");
  assert!(required.contains("Enter/D"), "{required}");
  assert!(!required.contains("Message"), "{required}");

  let mut failure = setup(ModelSetupPhase::Failed);
  failure.allowed_actions = vec![ModelSetupAction::Retry];
  failure.failure = Some(ModelSetupFailureView {
    code: "disk_full".into(),
    message: "free space required".into(),
    retryable: true,
  });
  let failed = screen(&state_with(failure), Locale::EnUs, 52, 12);
  assert!(failed.contains("phase: failed"), "{failed}");
  assert!(failed.contains("Enter/R/D retry"), "{failed}");
  assert!(failed.contains("code: disk_full"), "{failed}");
  assert!(failed.contains("message: free space required"), "{failed}");
  assert!(!failed.contains("Message"), "{failed}");
}

#[test]
fn closing_after_setup_receipt_is_visible_and_bilingual() {
  let state = state_with(setup(ModelSetupPhase::Required));
  for (locale, expected) in [
    (Locale::EnUs, "closing after command receipt"),
    (Locale::ZhCn, "收到命令回执后退出"),
  ] {
    let output = screen_with_context(
      &state,
      80,
      12,
      RenderContext {
        locale,
        setup_command_pending: true,
        setup_exit_pending: true,
        ..RenderContext::default()
      },
    );
    assert!(
      output.replace(' ', "").contains(&expected.replace(' ', "")),
      "{output}"
    );
  }
}

fn state_with(model_setup: ModelSetupView) -> TuiState {
  let mut state = TuiState::default();
  apply_remote_state(
    &mut state,
    &RemoteState {
      connection: ConnectionState::Connected {
        server_instance_id: "layout-daemon".into(),
      },
      snapshot: Some(VersionedRemoteSnapshot {
        server_instance_id: "layout-daemon".into(),
        revision: 1,
        snapshot: RuntimeSnapshot {
          phase: RuntimePhase::ModelSetupRequired,
          model_setup: Some(model_setup),
          ..RuntimeSnapshot::default()
        },
      }),
    },
  );
  state
}

fn setup(phase: ModelSetupPhase) -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase,
    managed_model: Some(ManagedModelView {
      id: "model-a".into(),
      expected_download_bytes: 2048,
      max_bytes: 4096,
      min_available_memory_bytes: 8192,
      sha256: "abc123".into(),
    }),
    operation_id: Some("operation-9".into()),
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions: vec![ModelSetupAction::Cancel],
  }
}

fn screen(state: &TuiState, locale: Locale, width: u16, height: u16) -> String {
  screen_with_context(
    state,
    width,
    height,
    RenderContext {
      locale,
      ..RenderContext::default()
    },
  )
}

fn screen_with_context(
  state: &TuiState,
  width: u16,
  height: u16,
  context: RenderContext,
) -> String {
  let backend = TestBackend::new(width, height);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| render_with_context(frame, state, context))
    .unwrap();
  let buffer = terminal.backend().buffer();
  (0..buffer.area.height)
    .map(|y| {
      (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}
