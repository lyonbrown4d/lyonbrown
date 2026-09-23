use std::{sync::Arc, thread};

use agent_core::{
  AgentEvent, RiskLevel, ToolCallId, ToolDefinition, ToolInvocation, ToolSource, TurnId,
};
use agent_kernel::Kernel;
use agent_tool::{ApprovalDecision, ApprovalPrompt};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use tokio::sync::broadcast;
use unicode_segmentation::UnicodeSegmentation;

use agent_cli::tui::runner::{
  Runtime, approval_decision, can_accept_input, control_c_key, edit_input, final_drain,
  next_conversation_scroll, quit_key, shutdown,
};
use agent_cli::tui::{TuiApprovalPrompt, TuiState};

#[test]
fn final_drain_consumes_events_queued_at_task_completion() {
  let (sender, receiver) = broadcast::channel(4);
  let mut receiver = Some(receiver);
  let mut state = TuiState::default();
  sender
    .send(AgentEvent::TurnCompleted {
      turn_id: TurnId::new(),
      answer: "last answer".into(),
      ts: Utc::now(),
    })
    .unwrap();

  final_drain(&mut receiver, &mut state);

  assert_eq!(state.transcript().last().unwrap().text, "last answer");
  assert_eq!(state.status(), "idle");
}

#[test]
fn input_remains_locked_until_task_join_returns_kernel() {
  let mut state = TuiState::default();
  state.reduce(AgentEvent::TurnCompleted {
    turn_id: TurnId::new(),
    answer: "rendered before join".into(),
    ts: Utc::now(),
  });
  assert_eq!(state.status(), "idle");
  assert!(!can_accept_input(true, false));
  assert!(!can_accept_input(true, true));
  assert!(!can_accept_input(false, false));
  assert!(can_accept_input(false, true));
}

#[test]
fn escape_quits_and_control_c_is_a_separate_interrupt_key() {
  assert!(quit_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
  assert!(!quit_key(KeyEvent::new(
    KeyCode::Char('c'),
    KeyModifiers::CONTROL
  )));
  assert!(control_c_key(KeyEvent::new(
    KeyCode::Char('c'),
    KeyModifiers::CONTROL
  )));
}

#[test]
fn input_editor_moves_and_deletes_on_unicode_boundaries() {
  let (input, cursor) = edit_input("a界b", 4, KeyCode::Left);
  assert_eq!((input.as_str(), cursor), ("a界b", 1));

  let (input, cursor) = edit_input(&input, cursor, KeyCode::Delete);
  assert_eq!((input.as_str(), cursor), ("ab", 1));

  let (input, cursor) = edit_input(&input, cursor, KeyCode::End);
  let (input, cursor) = edit_input(&input, cursor, KeyCode::Backspace);
  assert_eq!((input.as_str(), cursor), ("a", 1));

  let (input, cursor) = edit_input(&input, cursor, KeyCode::Home);
  let (input, cursor) = edit_input(&input, cursor, KeyCode::Char('好'));
  assert_eq!((input.as_str(), cursor), ("好a", 3));
}

#[test]
fn input_editor_moves_and_deletes_whole_grapheme_clusters() {
  let combining = "e\u{301}";
  let family = "👨‍👩‍👧‍👦";

  assert_eq!(edit_input(combining, combining.len(), KeyCode::Left).1, 0);
  assert_eq!(
    edit_input(combining, combining.len(), KeyCode::Backspace),
    (String::new(), 0)
  );
  assert_eq!(edit_input(family, 0, KeyCode::Right).1, family.len());
  assert_eq!(edit_input(family, 0, KeyCode::Delete), (String::new(), 0));
}

#[test]
fn insertion_that_joins_graphemes_keeps_cursor_at_a_visible_boundary() {
  let people = "👨👩";
  let between = "👨".len();
  let (joined, cursor) = edit_input(people, between, KeyCode::Char('\u{200d}'));

  assert_eq!(joined, "👨‍👩");
  assert!(is_grapheme_boundary(&joined, cursor));
  assert_eq!(cursor, joined.len());

  let (continued, cursor) = edit_input(&joined, cursor, KeyCode::Char('!'));
  assert_eq!(continued, "👨‍👩!");
  assert!(is_grapheme_boundary(&continued, cursor));
  assert_eq!(cursor, continued.len());
}

fn is_grapheme_boundary(value: &str, cursor: usize) -> bool {
  cursor == value.len()
    || value
      .grapheme_indices(true)
      .any(|(offset, _)| offset == cursor)
}

#[test]
fn approval_shortcuts_reject_command_modifiers_and_accept_plain_or_shifted_keys() {
  assert_eq!(
    approval_decision(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
    Some(ApprovalDecision::Allow)
  );
  assert_eq!(
    approval_decision(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
    Some(ApprovalDecision::AllowSession)
  );
  for (character, modifiers) in [
    ('y', KeyModifiers::CONTROL),
    ('a', KeyModifiers::CONTROL),
    ('d', KeyModifiers::CONTROL | KeyModifiers::SHIFT),
    ('n', KeyModifiers::ALT),
    ('y', KeyModifiers::SUPER),
  ] {
    assert_eq!(
      approval_decision(KeyEvent::new(KeyCode::Char(character), modifiers)),
      None
    );
  }
}

#[test]
fn conversation_scroll_uses_distance_from_bottom_and_page_down_resumes_following() {
  assert_eq!(next_conversation_scroll(0, 0, KeyCode::Up), 0);
  assert_eq!(next_conversation_scroll(0, 2, KeyCode::PageUp), 2);
  assert_eq!(next_conversation_scroll(2, 2, KeyCode::Up), 2);
  assert_eq!(next_conversation_scroll(2, 2, KeyCode::Down), 1);
  assert_eq!(next_conversation_scroll(1, 2, KeyCode::PageDown), 0);
}

#[tokio::test]
async fn shutdown_denies_approval_cancels_and_joins_turn_task() {
  let (prompt, approvals) = TuiApprovalPrompt::channel();
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
    arguments: json!({"command": "echo pending"}),
  };
  let prompt = Arc::new(prompt);
  let approval_worker = thread::spawn(move || prompt.prompt(&definition, &invocation));
  let pending = approvals.recv().unwrap();

  let kernel = Kernel::builder().build();
  let cancellation = kernel.cancellation_token();
  let task_cancellation = cancellation.clone();
  let task = tokio::spawn(async move {
    task_cancellation.cancelled().await;
    (kernel, Err(agent_core::AgentError::Cancelled))
  });
  let mut runtime = Runtime {
    state: TuiState::default(),
    kernel: None,
    events: None,
    task: Some(task),
    approval: Some(pending),
    approvals: Some(approvals),
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
  };

  shutdown(&mut runtime, &cancellation).await.unwrap();

  assert!(cancellation.is_cancelled());
  assert!(runtime.task.is_none());
  assert_eq!(approval_worker.join().unwrap(), ApprovalDecision::Deny);
}
