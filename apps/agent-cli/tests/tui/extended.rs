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
  let mut state = TuiState::default();
  for index in 0..24 {
    state.reduce(AgentEvent::ModelDelta {
      step_id: StepId::new(),
      delta: format!("message-{index}"),
    });
  }

  let output = scrolled_screen(&state, 6, 120, 12);
  assert!(!output.contains("message-23"));
  assert!(output.contains("6 from bottom"));
  assert!(output.contains("BUSY"));
  assert!(output.contains("↑↓ scroll"));
}

#[test]
fn narrow_layout_shows_idle_follow_state_and_scroll_keys() {
  let output = scrolled_screen(&TuiState::default(), 0, 52, 12);

  assert!(output.contains("IDLE"));
  assert!(output.contains("following"));
  assert!(output.contains("↑↓ scroll"));
  assert!(output.contains("←→ move"));
}

#[test]
fn composer_cursor_uses_unicode_display_width() {
  let mut state = TuiState::default();
  for character in "a界b".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }

  let (output, cursor) = screen_with_cursor(&state, 20, 8);
  assert!(output.contains("a界"), "{output}");
  assert!(output.contains('b'), "{output}");
  assert_eq!(cursor, Position::new(5, 5));
}

#[test]
fn composer_cursor_stays_on_grapheme_boundaries() {
  let mut state = TuiState::default();
  for character in "e\u{301}👨‍👩‍👧‍👦".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }
  state.apply_input_key(KeyCode::Left);

  let (_, cursor) = screen_with_cursor(&state, 20, 8);
  assert_eq!(cursor, Position::new(2, 5));
}

#[test]
fn composer_cursor_tracks_a_newly_joined_grapheme() {
  let mut state = TuiState::default();
  for character in "👨👩".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }
  state.apply_input_key(KeyCode::Left);
  state.apply_input_key(KeyCode::Char('\u{200d}'));

  let (_, joined_cursor) = screen_with_cursor(&state, 20, 8);
  assert_eq!(joined_cursor, Position::new(3, 5));

  state.apply_input_key(KeyCode::Char('!'));
  let (output, continued_cursor) = screen_with_cursor(&state, 20, 8);
  assert!(output.contains('!'));
  assert_eq!(continued_cursor, Position::new(4, 5));
}

#[test]
fn composer_viewport_follows_long_and_multiline_input() {
  let mut state = TuiState::default();
  for character in "hidden-prefix-界界界界\nmiddle\nsecond-line-with-visible-tail".chars() {
    state.apply_input_key(KeyCode::Char(character));
  }

  let (output, cursor) = screen_with_cursor(&state, 20, 8);
  assert!(output.contains("visible-tail"), "{output}");
  assert!(!output.contains("hidden-prefix"));
  assert_eq!(cursor.y, 6);
  assert!((1..=18).contains(&cursor.x));
}

#[test]
fn approval_bridge_sends_sanitized_request_and_returns_ui_decision() {
  let (prompt, requests) = TuiApprovalPrompt::channel();
  let prompt = Arc::new(prompt);
  let definition = ToolDefinition {
    name: "shell".into(),
    description: String::new(),
    input_schema: json!({"type": "object"}),
    source: ToolSource::BuiltIn,
    risk: RiskLevel::Execute,
    requires_secrets: vec![SecretRef::env("TOKEN")],
  };
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "shell".into(),
    arguments: json!({"command": "echo ready", "token": "secret:env:TOKEN"}),
  };
  let worker = thread::spawn(move || prompt.prompt(&definition, &invocation));

  let request = requests.recv().unwrap();
  assert_eq!(request.tool, "shell");
  assert_eq!(request.risk, RiskLevel::Execute);
  assert!(request.arguments.contains("echo ready"));
  assert_eq!(request.secret_references, ["env:TOKEN"]);
  assert!(!request.arguments.contains("plaintext-secret"));
  request.respond(ApprovalDecision::AllowSession);

  assert_eq!(worker.join().unwrap(), ApprovalDecision::AllowSession);
}

#[test]
fn approval_modal_scrolls_long_arguments_and_shows_position() {
  let (prompt, requests) = TuiApprovalPrompt::channel();
  let definition = ToolDefinition {
    name: "shell".into(),
    description: String::new(),
    input_schema: json!({"type": "object"}),
    source: ToolSource::BuiltIn,
    risk: RiskLevel::Execute,
    requires_secrets: vec![],
  };
  let long_command = (0..30)
    .map(|index| format!("argument-{index}"))
    .collect::<Vec<_>>()
    .join(" ");
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "shell".into(),
    arguments: json!({"command": format!("{long_command} TAIL-MARKER \u{1b}[31m")}),
  };
  let worker = thread::spawn(move || prompt.prompt(&definition, &invocation));
  let request = requests.recv().unwrap();

  let first = approval_screen(&TuiState::default(), &request, 0, 52, 14);
  let last = approval_screen(&TuiState::default(), &request, u16::MAX, 52, 14);
  assert!(first.contains("lines "));
  assert!(!first.contains("TAIL-MARKER"));
  assert!(last.contains("TAIL-MARKER"));
  assert!(!last.contains('\u{1b}'));

  request.respond(ApprovalDecision::Deny);
  assert_eq!(worker.join().unwrap(), ApprovalDecision::Deny);
}

#[test]
fn approval_modal_uses_full_minimum_screen_without_panicking() {
  let (prompt, requests) = TuiApprovalPrompt::channel();
  let definition = ToolDefinition {
    name: "shell".into(),
    description: String::new(),
    input_schema: json!({"type": "object"}),
    source: ToolSource::BuiltIn,
    risk: RiskLevel::Execute,
    requires_secrets: vec![],
  };
  let invocation = ToolInvocation {
    id: ToolCallId::new(),
    name: "shell".into(),
    arguments: json!({"command": "echo inspect"}),
  };
  let worker = thread::spawn(move || prompt.prompt(&definition, &invocation));
  let request = requests.recv().unwrap();

  let output = approval_screen(&TuiState::default(), &request, 0, 30, 8);
  assert!(output.contains("y/n/a/d"));
  assert!(output.contains("lines "));

  request.respond(ApprovalDecision::Deny);
  assert_eq!(worker.join().unwrap(), ApprovalDecision::Deny);
}
