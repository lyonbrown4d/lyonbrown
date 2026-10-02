use agent_app::{
  ManagedModelView, ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView,
  RuntimePhase, RuntimeSnapshot,
};
use agent_cli::tui::{
  Locale, RenderContext, TuiConnectionState, TuiState, command_catalog, render_with_context,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn model_setup_closes_overlay_so_it_does_not_restore_after_setup() {
  let mut state = ready_state();
  state.open_palette();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(ModelSetupView {
      source: ModelSetupSource::ManagedPreset,
      phase: ModelSetupPhase::Required,
      managed_model: Some(ManagedModelView {
        id: "small-model".into(),
        expected_download_bytes: 1024,
        max_bytes: 2048,
        min_available_memory_bytes: 4096,
        sha256: "abc".into(),
      }),
      operation_id: None,
      downloaded_bytes: 0,
      total_bytes: None,
      failure: None,
      allowed_actions: vec![ModelSetupAction::Download],
    }),
    ..RuntimeSnapshot::default()
  });

  let output = screen(&state, Locale::EnUs, 90, 20);

  assert!(output.contains("MODEL SETUP"), "{output}");
  assert!(!output.contains("COMMANDS"), "{output}");
  assert!(!state.overlay().is_open());
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    ..RuntimeSnapshot::default()
  });
  assert!(!state.overlay().is_open());
}

#[test]
fn palette_and_help_render_in_both_locales_at_supported_sizes() {
  for locale in [Locale::EnUs, Locale::ZhCn] {
    for (width, height) in [(30, 8), (52, 12), (90, 20)] {
      let mut palette = ready_state();
      palette.apply_connection(TuiConnectionState::Unavailable, Some("offline".into()));
      palette.open_palette();
      let palette_screen = screen(&palette, locale, width, height);
      let compact_palette = palette_screen.replace(' ', "");
      assert!(
        compact_palette.contains(if locale == Locale::EnUs {
          "COMMANDS"
        } else {
          "命令"
        }),
        "{width}x{height}\n{palette_screen}"
      );
      assert!(palette_screen.contains("/sessions"), "{palette_screen}");
      assert!(
        palette_screen
          .replace(' ', "")
          .contains(if locale == Locale::EnUs {
            "disabled"
          } else {
            "已禁用"
          }),
        "{palette_screen}"
      );

      let mut help = ready_state();
      help.open_help();
      let help_screen = screen(&help, locale, width, height);
      let compact_help = help_screen.replace(' ', "");
      assert!(
        compact_help.contains(if locale == Locale::EnUs {
          "HELP"
        } else {
          "帮助"
        }),
        "{width}x{height}\n{help_screen}"
      );
      assert!(help_screen.contains("Ctrl-P"), "{help_screen}");
      assert!(help_screen.contains("Ctrl-C"), "{help_screen}");
      assert!(help_screen.contains("Ctrl-D"), "{help_screen}");
    }
  }
}

#[test]
fn palette_empty_search_has_an_explicit_bilingual_state() {
  for (locale, expected) in [
    (Locale::EnUs, "No matching commands"),
    (Locale::ZhCn, "没有匹配的命令"),
  ] {
    let mut state = ready_state();
    state.open_palette();
    for character in "no-such-command".chars() {
      state.apply_overlay_key(
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
        12,
      );
    }
    let output = screen(&state, locale, 52, 12);
    assert!(
      output.replace(' ', "").contains(&expected.replace(' ', "")),
      "{output}"
    );
  }
}

#[test]
fn disconnected_status_advertises_local_help_and_cached_inspector() {
  for locale in [Locale::EnUs, Locale::ZhCn] {
    let mut state = ready_state();
    state.apply_connection(TuiConnectionState::Unavailable, Some("offline".into()));

    let output = screen(&state, locale, 140, 12);

    assert!(output.contains("Ctrl-P"), "{output}");
    assert!(output.contains("/language"), "{output}");
    assert!(output.contains("/quit"), "{output}");
    assert!(output.contains("Ctrl-O"), "{output}");
  }
}

#[test]
fn help_end_lists_every_catalog_command_with_real_availability() {
  for locale in [Locale::EnUs, Locale::ZhCn] {
    let mut state = ready_state();
    state.open_help();
    state.apply_overlay_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), 12);

    let output = screen(&state, locale, 52, 12);

    for command in command_catalog() {
      assert!(
        output.contains(command.alias),
        "{}\n{output}",
        command.alias
      );
    }
    assert!(
      output.replace(' ', "").contains(if locale == Locale::EnUs {
        "disabled"
      } else {
        "已禁用"
      }),
      "{output}"
    );
  }
}

fn ready_state() -> TuiState {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
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
