use agent_app::{ActivityPhase, ActivityView, RuntimePhase, RuntimeSnapshot, TurnStatus, TurnView};
use agent_cli::tui::{Locale, RenderContext, TuiConnectionState, TuiState, render_with_context};
use agent_core::{StepId, TurnId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn inspector_is_responsive_and_bilingual_at_supported_sizes() {
  for (locale, title, list, detail) in [
    (Locale::EnUs, "ACTIVITY INSPECTOR", "STEPS", "DETAIL"),
    (Locale::ZhCn, "活动检查器", "步骤", "详情"),
  ] {
    for (width, height) in [(30, 8), (52, 12), (90, 20)] {
      let mut state = populated_state();
      state.open_activity();
      let list_screen = screen(&state, locale, width, height);
      assert!(
        compact(&list_screen).contains(&compact(title)),
        "{width}x{height}\n{list_screen}"
      );
      assert!(
        compact(&list_screen).contains(&compact(list)),
        "{list_screen}"
      );
      assert!(
        list_screen.contains('›'),
        "selection needs a non-color marker\n{list_screen}"
      );

      state.apply_overlay_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), height);
      let detail_screen = screen(&state, locale, width, height);
      assert!(
        compact(&detail_screen).contains(&compact(detail)),
        "{width}x{height}\n{detail_screen}"
      );
      assert!(detail_screen.contains("selected/model"), "{detail_screen}");
    }
  }
}

#[test]
fn stale_cached_activity_is_visible_and_explicitly_marked() {
  for (locale, stale) in [(Locale::EnUs, "STALE"), (Locale::ZhCn, "过期缓存")] {
    let mut state = populated_state();
    state.apply_connection(TuiConnectionState::Unavailable, Some("offline".into()));
    state.open_activity();

    let output = screen(&state, locale, 90, 20);

    assert!(compact(&output).contains(&compact(stale)), "{output}");
    if locale == Locale::ZhCn {
      assert!(
        !compact(&output).contains(&compact("状态已过期缓存")),
        "{output}"
      );
    }
    assert!(output.contains("selected/model"), "{output}");
  }
}

#[test]
fn help_and_status_discover_ctrl_o_in_both_locales() {
  for locale in [Locale::EnUs, Locale::ZhCn] {
    let mut help = populated_state();
    help.open_help();
    let help_screen = screen(&help, locale, 52, 12);
    assert!(help_screen.contains("Ctrl-O"), "{help_screen}");

    let status_screen = screen(&populated_state(), locale, 100, 12);
    assert!(status_screen.contains("Ctrl-O"), "{status_screen}");
  }
}

#[test]
fn resize_and_large_scroll_never_blank_the_detail() {
  let mut state = populated_state();
  state.open_activity();
  state.apply_overlay_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20);
  state.apply_overlay_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), 20);

  for (width, height) in [(90, 20), (52, 12), (30, 8), (52, 12), (90, 20)] {
    let output = screen(&state, Locale::EnUs, width, height);
    assert!(output.contains("DETAIL"), "{width}x{height}\n{output}");
    assert!(output.contains("wrapped"), "{width}x{height}\n{output}");
  }
}

#[test]
fn too_small_surface_keeps_existing_guard() {
  let mut state = populated_state();
  state.open_activity();
  let output = screen(&state, Locale::EnUs, 29, 7);
  assert!(output.contains("Terminal too small"), "{output}");
  assert!(!output.contains("ACTIVITY INSPECTOR"), "{output}");
}

#[test]
fn disappearing_last_target_has_an_explicit_empty_detail_state() {
  for (locale, empty) in [
    (Locale::EnUs, "No step activity"),
    (Locale::ZhCn, "没有步骤活动"),
  ] {
    let mut state = populated_state();
    state.open_activity();
    state.apply_overlay_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 8);
    state.sync_snapshot(&RuntimeSnapshot {
      phase: RuntimePhase::Ready,
      ..RuntimeSnapshot::default()
    });

    let output = screen(&state, locale, 30, 8);

    assert!(compact(&output).contains(&compact(empty)), "{output}");
  }
}

#[test]
fn inspector_title_always_leads_with_localized_freshness() {
  for (locale, fresh, stale) in [
    (Locale::EnUs, "FRESH", "STALE CACHED"),
    (Locale::ZhCn, "状态已同步", "过期缓存"),
  ] {
    for (width, height) in [(30, 8), (52, 12), (90, 20)] {
      let mut state = populated_state();
      state.open_activity();
      let fresh_screen = screen(&state, locale, width, height);
      assert!(
        compact(&fresh_screen).contains(&compact(fresh)),
        "{width}x{height}\n{fresh_screen}"
      );

      state.apply_connection(TuiConnectionState::Unavailable, Some("offline".into()));
      let stale_screen = screen(&state, locale, width, height);
      assert!(
        compact(&stale_screen).contains(&compact(stale)),
        "{width}x{height}\n{stale_screen}"
      );
    }
  }
}

#[test]
fn responsive_modes_have_distinct_pane_geometry() {
  let mut wide = populated_state();
  wide.open_activity();
  let wide_screen = screen(&wide, Locale::EnUs, 90, 20);
  let wide_steps = position(&wide_screen, "STEPS").unwrap();
  let wide_detail = position(&wide_screen, "DETAIL").unwrap();
  assert_eq!(
    wide_steps.0, wide_detail.0,
    "dual panes must share a row\n{wide_screen}"
  );
  assert!(wide_detail.1 > wide_steps.1 + 20, "{wide_screen}");

  let mut stacked = populated_state();
  stacked.open_activity();
  let stacked_screen = screen(&stacked, Locale::EnUs, 52, 12);
  let stacked_steps = position(&stacked_screen, "STEPS").unwrap();
  let stacked_detail = position(&stacked_screen, "DETAIL").unwrap();
  assert!(
    stacked_detail.0 > stacked_steps.0,
    "panes must stack\n{stacked_screen}"
  );
  assert_eq!(stacked_steps.1, stacked_detail.1, "{stacked_screen}");

  let mut single = populated_state();
  single.open_activity();
  let list_screen = screen(&single, Locale::EnUs, 30, 8);
  assert!(position(&list_screen, "STEPS").is_some(), "{list_screen}");
  assert!(position(&list_screen, "DETAIL").is_none(), "{list_screen}");
  single.apply_overlay_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 8);
  let detail_screen = screen(&single, Locale::EnUs, 30, 8);
  assert!(
    position(&detail_screen, "STEPS").is_none(),
    "{detail_screen}"
  );
  assert!(
    position(&detail_screen, "DETAIL").is_some(),
    "{detail_screen}"
  );
}

fn populated_state() -> TuiState {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    turns: vec![TurnView {
      turn_id: TurnId::new(),
      user_text: "inspect".into(),
      steering: vec![],
      assistant_text: String::new(),
      status: TurnStatus::Running,
      activities: (0..8)
        .map(|index| ActivityView {
          step_id: StepId::new(),
          phase: ActivityPhase::Completed,
          route: Some(agent_app::RouteSummary {
            selected: agent_app::ModelRoute {
              provider: "selected".into(),
              model: "model".into(),
            },
            reason: "policy route".into(),
            fallbacks: vec![],
          }),
          context: None,
          assistant_text: format!("assistant detail {index} {}", "wrapped ".repeat(20)),
          usage: None,
          tools: vec![],
          outcome: None,
        })
        .collect(),
    }],
    ..RuntimeSnapshot::default()
  });
  state
}

fn screen(state: &TuiState, locale: Locale, width: u16, height: u16) -> String {
  let backend = TestBackend::new(width, height);
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

fn compact(value: &str) -> String {
  value
    .chars()
    .filter(|character| !character.is_whitespace())
    .collect()
}

fn position(screen: &str, needle: &str) -> Option<(usize, usize)> {
  screen
    .lines()
    .enumerate()
    .find_map(|(row, line)| line.find(needle).map(|column| (row, column)))
}
