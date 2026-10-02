use agent_app::{RuntimePhase, RuntimeSnapshot};
use agent_cli::tui::runner::can_accept_input;
use agent_cli::tui::{Locale, RenderContext, TuiConnectionState, TuiState, render_with_context};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn model_setup_snapshot_keeps_tui_connected_but_disables_submission() {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    ..RuntimeSnapshot::default()
  });

  assert_eq!(state.connection(), TuiConnectionState::Connected);
  assert!(!state.can_submit());
  assert_eq!(state.status(), "model setup required");
  assert!(render_text(&state, Locale::EnUs).contains("MODEL SETUP"));
  assert!(
    render_text(&state, Locale::ZhCn)
      .replace(' ', "")
      .contains("需要配置模型")
  );
}

#[test]
fn reconnect_keeps_cached_snapshot_stale_until_a_fresh_projection_arrives() {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    ..RuntimeSnapshot::default()
  });
  assert!(state.can_submit());

  state.apply_connection(
    TuiConnectionState::Reconnecting,
    Some("daemon connection lost".into()),
  );
  assert_eq!(state.connection(), TuiConnectionState::Reconnecting);
  assert!(state.is_snapshot_stale());
  assert!(!state.can_submit());
  assert_eq!(state.connection_error(), Some("daemon connection lost"));

  state.apply_connection(TuiConnectionState::Connected, None);
  assert!(state.is_snapshot_stale());
  assert!(!state.can_submit());

  state.mark_snapshot_fresh();
  assert!(!state.is_snapshot_stale());
  assert!(state.can_submit());
}

#[test]
fn incompatible_connection_never_accepts_commands() {
  let mut state = TuiState::default();
  state.apply_connection(
    TuiConnectionState::Incompatible,
    Some("protocol version mismatch".into()),
  );

  assert!(!state.can_submit());
  assert_eq!(state.connection(), TuiConnectionState::Incompatible);
  assert_eq!(state.connection_error(), Some("protocol version mismatch"));
  assert!(!can_accept_input(true, false));
}

#[test]
fn reconnecting_status_is_visible_in_english_and_chinese() {
  let mut state = TuiState::default();
  state.apply_connection(
    TuiConnectionState::Reconnecting,
    Some("daemon connection lost".into()),
  );

  let english = render_text(&state, Locale::EnUs);
  assert!(english.contains("RECONNECTING"));
  assert!(english.contains("stale"));
  assert!(english.contains("daemon connection lost"));

  let chinese = render_text(&state, Locale::ZhCn);
  let compact = chinese.replace(' ', "");
  assert!(compact.contains("重新连接中"), "{chinese}");
  assert!(compact.contains("状态已过期"), "{chinese}");
}

fn render_text(state: &TuiState, locale: Locale) -> String {
  let backend = TestBackend::new(96, 18);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| {
      render_with_context(
        frame,
        state,
        RenderContext {
          locale,
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
