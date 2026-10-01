use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentClient, RuntimePhase, RuntimeSnapshot, TurnStatus, TurnView, channel};
use agent_core::{AgentEvent, TurnId};
use agent_tool::ApprovalDecision;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::broadcast;
use unicode_segmentation::UnicodeSegmentation;

use agent_cli::tui::TuiState;
use agent_cli::tui::runner::{
  ComposerIntent, Runtime, approval_decision, can_accept_input, composer_intent, control_c_key,
  edit_input, final_drain, next_conversation_scroll, quit_key, shutdown,
};

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
fn composer_accepts_input_while_a_turn_is_active() {
  let mut state = TuiState::default();
  state.reduce(AgentEvent::TurnCompleted {
    turn_id: TurnId::new(),
    answer: "rendered before join".into(),
    ts: Utc::now(),
  });
  assert_eq!(state.status(), "idle");
  assert!(can_accept_input(true, false));
  assert!(can_accept_input(true, true));
  assert!(!can_accept_input(false, false));
  assert!(can_accept_input(false, true));
}

#[test]
fn tui_state_projects_the_shared_runtime_snapshot() {
  let mut snapshot = RuntimeSnapshot {
    phase: RuntimePhase::Streaming,
    ..RuntimeSnapshot::default()
  };
  snapshot.turns.push(TurnView {
    turn_id: TurnId::new(),
    user_text: "inspect".into(),
    steering: vec!["focus on tests".into()],
    assistant_text: "working".into(),
    status: TurnStatus::Running,
    activities: vec![],
  });
  let mut state = TuiState::default();

  state.sync_snapshot(&snapshot);

  assert!(state.is_busy());
  assert_eq!(state.status(), "streaming");
  assert_eq!(state.transcript().len(), 3);
  assert_eq!(state.transcript()[1].text, "focus on tests");
  assert_eq!(state.transcript()[2].text, "working");
}

#[test]
fn enter_intent_distinguishes_submit_steer_and_queue() {
  assert_eq!(
    composer_intent(false, KeyModifiers::NONE),
    ComposerIntent::StartTurn
  );
  assert_eq!(
    composer_intent(true, KeyModifiers::NONE),
    ComposerIntent::SteerCurrentTurn
  );
  assert_eq!(
    composer_intent(true, KeyModifiers::ALT),
    ComposerIntent::QueueFollowUp
  );
}

#[test]
fn explicit_quit_and_control_c_are_separate_from_escape() {
  assert!(!quit_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
  assert!(quit_key(KeyEvent::new(
    KeyCode::Char('d'),
    KeyModifiers::CONTROL
  )));
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
    approval_decision(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    Some(ApprovalDecision::Deny)
  );
  assert_eq!(
    approval_decision(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    Some(ApprovalDecision::Deny)
  );
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
async fn shutdown_detaches_without_cancelling_the_remote_runtime() {
  let (controller, mut endpoint) = channel(
    RuntimeSnapshot {
      phase: RuntimePhase::Streaming,
      ..RuntimeSnapshot::default()
    },
    4,
  );
  let client: Arc<dyn AgentClient> = Arc::new(controller);
  let observer = client.clone();
  let snapshots = client.subscribe();
  let mut runtime = Runtime {
    state: TuiState::default(),
    client: Some(client),
    snapshots,
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
  };

  tokio::time::timeout(Duration::from_millis(25), shutdown(&mut runtime))
    .await
    .expect("shutdown should detach without waiting for a command response")
    .unwrap();

  assert!(runtime.client.is_none());
  assert!(
    tokio::time::timeout(Duration::from_millis(25), endpoint.recv())
      .await
      .is_err()
  );
  drop(observer);
}
