use agent_app::{
  ManagedModelView, ModelSetupAction, ModelSetupFailureView, ModelSetupPhase, ModelSetupSource,
  ModelSetupView, RuntimePhase, RuntimeSnapshot,
};
use agent_cli::tui::{
  Locale, RenderContext, TuiState, format_model_setup_progress, render_with_context,
};
use ratatui::{Terminal, backend::TestBackend, text::Line};

#[test]
fn runtime_setup_phase_never_allows_prompt_submission_without_an_active_view() {
  for model_setup in [None, Some(setup(ModelSetupPhase::Ready))] {
    let mut state = TuiState::default();
    state.sync_snapshot(&RuntimeSnapshot {
      phase: RuntimePhase::ModelSetupRequired,
      model_setup,
      ..RuntimeSnapshot::default()
    });
    assert!(!state.can_submit());
  }
}

#[test]
fn ready_setup_is_shown_until_the_runtime_phase_becomes_ready() {
  let view = setup(ModelSetupPhase::Ready);
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(view.clone()),
    ..RuntimeSnapshot::default()
  });
  let transitioning = screen(&state, Locale::EnUs, 80);
  assert!(transitioning.contains("MODEL SETUP"), "{transitioning}");
  assert!(transitioning.contains("phase: ready"), "{transitioning}");

  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    model_setup: Some(view),
    ..RuntimeSnapshot::default()
  });
  let ready = screen(&state, Locale::EnUs, 80);
  assert!(ready.contains("ACTIVITY"), "{ready}");
  assert!(!ready.contains("model ID: model-a"), "{ready}");
}

#[test]
fn progress_bar_adapts_to_width_for_known_and_unknown_totals() {
  let mut known = setup(ModelSetupPhase::Downloading);
  known.downloaded_bytes = 512;
  known.total_bytes = Some(1024);
  let mut state = state_with(known);
  let narrow = progress_line(&screen(&state, Locale::EnUs, 48));
  let wide = progress_line(&screen(&state, Locale::EnUs, 100));
  assert!(narrow.contains("[#"), "{narrow}");
  assert!(narrow.contains("-] 50% · 512 B / 1.00 KiB"), "{narrow}");
  assert!(bar_width(&wide) > bar_width(&narrow), "{narrow}\n{wide}");

  let mut unknown = setup(ModelSetupPhase::Downloading);
  unknown.downloaded_bytes = 1536;
  unknown.total_bytes = None;
  state = state_with(unknown);
  let line = progress_line(&screen(&state, Locale::EnUs, 72));
  assert!(line.contains("[~"), "{line}");
  assert!(
    line.contains("unknown total · downloaded 1.50 KiB"),
    "{line}"
  );
}

#[test]
fn chinese_localizes_labels_but_preserves_technical_values() {
  let mut failed = setup(ModelSetupPhase::Failed);
  failed.failure = Some(ModelSetupFailureView {
    code: "checksum_mismatch".into(),
    message: "digest mismatch".into(),
    retryable: true,
  });
  let output = screen(&state_with(failed), Locale::ZhCn, 100).replace(' ', "");

  for expected in [
    "模型ID:model-a",
    "操作ID:operation-9",
    "错误代码:checksum_mismatch",
    "错误信息:digestmismatch",
  ] {
    assert!(output.contains(expected), "{output}");
  }
  for forbidden in ["modelID:", "operationID:", "code:", "message:"] {
    assert!(!output.contains(forbidden), "{output}");
  }
}

#[test]
fn progress_bar_uses_terminal_columns_and_omits_it_when_space_is_insufficient() {
  let mut view = setup(ModelSetupPhase::Downloading);
  view.downloaded_bytes = 512;
  view.total_bytes = Some(1024);
  let state = state_with(view);
  let english = progress_line(&screen(&state, Locale::EnUs, 80));
  let chinese = progress_line(&screen(&state, Locale::ZhCn, 80));
  assert!(
    bar_width(&chinese) > bar_width(&english),
    "{english}\n{chinese}"
  );

  let minimum = screen(&state, Locale::ZhCn, 30);
  assert!(!minimum.contains('['), "{minimum}");
}

#[test]
fn pending_hint_keeps_escape_exit_for_terminal_setup_phases() {
  for phase in [
    ModelSetupPhase::Required,
    ModelSetupPhase::Failed,
    ModelSetupPhase::Cancelled,
  ] {
    let state = state_with(setup(phase));
    let output = screen_with_pending(&state, Locale::EnUs, 80);
    assert!(
      output.contains("command in progress · Q/Esc exit"),
      "{phase:?}: {output}"
    );
  }
}

#[test]
fn minimum_width_progress_is_one_complete_terminal_width_line() {
  for locale in [Locale::EnUs, Locale::ZhCn] {
    for total in [Some(1024), None] {
      let mut view = setup(ModelSetupPhase::Downloading);
      view.downloaded_bytes = 512;
      view.total_bytes = total;
      let line = format_model_setup_progress(&view, locale, 28);
      assert!(!line.contains('\n'), "{locale:?}: {line:?}");
      assert!(
        Line::from(line.as_str()).width() <= 28,
        "{locale:?}: {line}"
      );
      assert!(line.contains("512 B"), "{locale:?}: {line}");
      if total.is_some() {
        assert!(line.contains("50%"), "{locale:?}: {line}");
      } else {
        assert!(line.contains('…'), "{locale:?}: {line}");
      }
    }
  }
}

fn state_with(model_setup: ModelSetupView) -> TuiState {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(model_setup),
    ..RuntimeSnapshot::default()
  });
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

fn screen(state: &TuiState, locale: Locale, width: u16) -> String {
  screen_at_with_context(state, locale, width, 22, false)
}

fn screen_with_pending(state: &TuiState, locale: Locale, width: u16) -> String {
  screen_with_context(state, locale, width, true)
}

fn screen_with_context(state: &TuiState, locale: Locale, width: u16, pending: bool) -> String {
  screen_at_with_context(state, locale, width, 22, pending)
}

fn screen_at_with_context(
  state: &TuiState,
  locale: Locale,
  width: u16,
  height: u16,
  pending: bool,
) -> String {
  let backend = TestBackend::new(width, height);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| {
      render_with_context(
        frame,
        state,
        RenderContext {
          locale,
          setup_command_pending: pending,
          ..RenderContext::default()
        },
      )
    })
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

fn progress_line(screen: &str) -> String {
  screen
    .lines()
    .find(|line| line.contains('['))
    .unwrap_or_default()
    .to_owned()
}

fn bar_width(line: &str) -> usize {
  let start = line.find('[').unwrap();
  let end = line[start..].find(']').unwrap() + start;
  end - start - 1
}
