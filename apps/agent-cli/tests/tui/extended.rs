use super::*;

#[test]
fn wide_conversation_follows_the_newest_streaming_text() {
  let mut state = TuiState::default();
  for index in 0..20 {
    state.reduce(AgentEvent::ModelDelta {
      step_id: StepId::new(),
      delta: format!("older message {index}"),
    });
  }
  state.reduce(AgentEvent::ModelDelta {
    step_id: StepId::new(),
    delta: "NEWEST-WIDE-MARKER".into(),
  });

  assert!(screen(&state, 120, 12).contains("NEWEST-WIDE-MARKER"));
}

#[test]
fn narrow_conversation_follows_wrapped_newest_streaming_text() {
  let mut state = TuiState::default();
  for index in 0..20 {
    state.reduce(AgentEvent::ModelDelta {
      step_id: StepId::new(),
      delta: format!("older narrow message {index}"),
    });
  }
  state.reduce(AgentEvent::ModelDelta {
    step_id: StepId::new(),
    delta: "NEWEST-NARROW-MARKER".into(),
  });

  assert!(screen(&state, 72, 12).contains("NEWEST-NARROW-MARKER"));
}

#[test]
fn narrow_conversation_follows_newest_text_after_word_boundary_wraps() {
  let mut state = TuiState::default();
  let multi_word = "abcdefghijklmnopqrst uvwxyzabcdefghijklmn opqrstuvwxyzabcdefgh";
  for _ in 0..8 {
    state.reduce(AgentEvent::ModelDelta {
      step_id: StepId::new(),
      delta: multi_word.into(),
    });
  }
  state.reduce(AgentEvent::ModelDelta {
    step_id: StepId::new(),
    delta: "NEWEST-WORD-WRAP-MARKER".into(),
  });

  assert!(screen(&state, 40, 12).contains("NEWEST-WORD-WRAP-MARKER"));
}

#[test]
fn manual_scroll_hides_newest_text_and_reports_position_in_wide_layout() {
  let mut state = ready_state();
  for index in 0..24 {
    state.reduce(AgentEvent::ModelDelta {
      step_id: StepId::new(),
      delta: format!("message-{index}"),
    });
  }

  let output = scrolled_screen(&state, 6, 120, 12);
  assert!(!output.contains("message-23"));
  assert!(output.contains("6 from bottom"));
  assert!(output.contains("RUNNING"));
  assert!(output.contains("↑↓ scroll"));
}

#[test]
fn narrow_layout_shows_idle_follow_state_and_scroll_keys() {
  let output = scrolled_screen(&ready_state(), 0, 52, 12);

  assert!(output.contains("READY"));
  assert!(output.contains("following"));
  assert!(output.contains("↑↓ scroll"));
  assert!(output.contains("Enter send now"));
}

#[test]
fn running_composer_keeps_input_visible_and_explains_steer_and_queue() {
  let mut state = ready_state();
  state.reduce(AgentEvent::ModelDelta {
    step_id: StepId::new(),
    delta: "working".into(),
  });
  for character in "adjust the plan".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }

  let (output, cursor) = screen_with_cursor(&state, 90, 14);
  assert!(output.contains("adjust the plan"), "{output}");
  assert!(output.contains("Enter steer now"), "{output}");
  assert!(output.contains("Alt-Enter queue next"), "{output}");
  assert!(!output.contains("input locked"), "{output}");
  assert!(cursor.x > 0);
}

#[test]
fn supported_responsive_sizes_render_without_panicking_in_both_locales() {
  for locale in [Locale::EnUs, Locale::ZhCn] {
    for (width, height) in [(30, 8), (52, 12), (90, 16), (120, 28)] {
      let output = localized_screen(&TuiState::default(), locale, width, height);
      assert!(!output.is_empty(), "empty screen at {width}x{height}");
    }
  }
}

#[test]
fn composer_cursor_uses_unicode_display_width() {
  let mut state = ready_state();
  for character in "a界b".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }

  let (output, cursor) = screen_with_cursor(&state, 30, 8);
  assert!(output.contains("a界"), "{output}");
  assert!(output.contains('b'), "{output}");
  assert_eq!(cursor.x, 5);
  assert!((4..=6).contains(&cursor.y));
}

#[test]
fn composer_cursor_stays_on_grapheme_boundaries() {
  let mut state = ready_state();
  for character in "e\u{301}👨‍👩‍👧‍👦".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }
  state.apply_input_key(KeyCode::Left);

  let (_, cursor) = screen_with_cursor(&state, 30, 8);
  assert_eq!(cursor.x, 2);
  assert!((4..=6).contains(&cursor.y));
}

#[test]
fn composer_cursor_tracks_a_newly_joined_grapheme() {
  let mut state = ready_state();
  for character in "👨👩".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }
  state.apply_input_key(KeyCode::Left);
  state.apply_input_key(KeyCode::Char('\u{200d}'));

  let (_, joined_cursor) = screen_with_cursor(&state, 30, 8);
  assert_eq!(joined_cursor.x, 3);
  assert!((4..=6).contains(&joined_cursor.y));

  state.apply_input_key(KeyCode::Char('!'));
  let (output, continued_cursor) = screen_with_cursor(&state, 30, 8);
  assert!(output.contains('!'));
  assert_eq!(continued_cursor.x, 4);
  assert!((4..=6).contains(&continued_cursor.y));
}

#[test]
fn composer_viewport_follows_long_and_multiline_input() {
  let mut state = ready_state();
  for character in "hidden-prefix-界界界界\nmiddle\nsecond-line-with-visible-tail".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }

  let (output, cursor) = screen_with_cursor(&state, 30, 8);
  assert!(output.contains("visible-tail"), "{output}");
  assert!(!output.contains("hidden-prefix"));
  assert!((4..=6).contains(&cursor.y));
  assert!((1..=28).contains(&cursor.x));
}

#[test]
fn approval_view_keeps_the_typed_safety_preview() {
  let request = approval_request("echo ready", vec![SecretRef::env("TOKEN")]);

  assert_eq!(request.preview.tool, "shell");
  assert_eq!(request.preview.risk, RiskLevel::Execute);
  assert_eq!(request.preview.target, "echo ready");
  assert_eq!(request.preview.required_secrets, [SecretRef::env("TOKEN")]);
  assert_eq!(request.preview.sandbox, SandboxStatus::HostUnsandboxed);
}

#[test]
fn approval_modal_scrolls_long_arguments_and_shows_position() {
  let long_command = (0..30)
    .map(|index| format!("argument-{index}"))
    .collect::<Vec<_>>()
    .join(" ");
  let request = approval_request(&format!("{long_command} TAIL-MARKER \u{1b}[31m"), vec![]);

  let first = approval_screen(&TuiState::default(), &request, 0, 52, 14);
  let last = approval_screen(&TuiState::default(), &request, u16::MAX, 52, 14);
  assert!(first.contains("lines "), "{first}");
  assert!(!first.contains("TAIL-MARKER"));
  assert!(last.contains("TAIL-MARKER"));
  assert!(!last.contains('\u{1b}'));
}

#[test]
fn approval_modal_uses_full_minimum_screen_without_panicking() {
  let request = approval_request("echo inspect", vec![]);

  let output = approval_screen(&TuiState::default(), &request, 0, 30, 8);
  assert!(output.contains("y allow"));
  assert!(output.contains("lines "), "{output}");
}

fn approval_request(command: &str, required_secrets: Vec<SecretRef>) -> ApprovalRequest {
  ApprovalRequest {
    request_id: ToolCallId::new(),
    preview: ToolPreview {
      tool: "shell".into(),
      action: "execute command".into(),
      target: command.into(),
      impact: "runs a command on the host".into(),
      risk: RiskLevel::Execute,
      sandbox: SandboxStatus::HostUnsandboxed,
      working_directory: None,
      required_secrets,
    },
  }
}
