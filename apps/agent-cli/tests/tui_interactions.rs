use agent_cli::{
  backend::{Action, Update},
  input,
  model::{App, Connection, ContextPanel, Session},
  picker::Picker,
  update,
};
use agent_core::events::{AgentEvent, ExecutionPhase, ModelIdentity, QueueKind};
use agent_protocol::v1;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

fn app() -> App {
  App {
    connection: Connection::Connected,
    sessions: vec![Session {
      id: "session".into(),
      title: "Conversation".into(),
    }],
    generation: 7,
    ..App::default()
  }
}

fn key(app: &mut App, actions: &mpsc::Sender<Action>, code: KeyCode, control: bool) -> bool {
  input::handle(
    app,
    Event::Key(KeyEvent::new(
      code,
      if control {
        KeyModifiers::CONTROL
      } else {
        KeyModifiers::NONE
      },
    )),
    actions,
  )
  .expect("handle terminal key")
}

fn acknowledge(app: &mut App, prompt: &str) {
  update::apply(
    app,
    Update::Submitted {
      session: "session".into(),
      prompt: prompt.into(),
      generation: 7,
    },
  );
}

#[test]
fn acknowledgement_owns_history_and_filtered_selection_only_refills_the_composer() {
  let mut app = app();
  let (actions, mut receiver) = mpsc::channel(8);
  app.input = "older accepted prompt".into();
  key(&mut app, &actions, KeyCode::Enter, false);
  assert!(
    matches!(receiver.try_recv(), Ok(Action::Submit { prompt, .. }) if prompt == "older accepted prompt")
  );
  assert!(app.interaction.history.entries("session").is_empty());
  assert_eq!(app.input.text(), "older accepted prompt");
  assert_eq!(
    app.drafts.get("session").map(String::as_str),
    Some("older accepted prompt")
  );
  // A late acknowledgement must not erase typing that began after submission.
  app.input = "new draft".into();
  acknowledge(&mut app, "older accepted prompt");
  assert_eq!(app.input.text(), "new draft");
  acknowledge(&mut app, "newer accepted prompt");
  key(&mut app, &actions, KeyCode::Char('p'), true);
  key(&mut app, &actions, KeyCode::Down, false);
  for ch in "older".chars() {
    key(&mut app, &actions, KeyCode::Char(ch), false);
  }
  assert_eq!(
    app
      .picker
      .as_ref()
      .expect("history picker")
      .visible_indices(),
    vec![1]
  );
  key(&mut app, &actions, KeyCode::Backspace, false);
  key(&mut app, &actions, KeyCode::Up, false);
  key(&mut app, &actions, KeyCode::Enter, false);
  assert!(app.picker.is_none());
  assert_eq!(app.input.text(), "older accepted prompt");
  assert_eq!(app.interaction.history.entries("session").len(), 2);
  assert!(
    receiver.try_recv().is_err(),
    "history confirmation never dispatches an action"
  );
}

#[test]
fn global_exit_remains_available_inside_picker_and_diff() {
  let mut app = app();
  let (actions, mut receiver) = mpsc::channel(2);
  app.input = "unsent draft".into();
  app.open_picker(Picker::loading("Loading"));
  assert!(key(&mut app, &actions, KeyCode::Char('c'), true));
  app.close_picker();
  app.open_diff("file.rs".into(), Default::default());
  assert!(key(&mut app, &actions, KeyCode::Char('d'), true));
  assert_eq!(app.input.text(), "unsent draft");
  assert!(
    receiver.try_recv().is_err(),
    "client exit does not cancel daemon work"
  );
}

fn approvals(app: &mut App, rows: &[(&str, i64)]) {
  update::apply(
    app,
    Update::Approvals {
      session: "session".into(),
      generation: 7,
      approvals: rows
        .iter()
        .map(|(id, expiry)| v1::Approval {
          id: (*id).into(),
          run_id: "run".into(),
          tool: "write_file".into(),
          arguments_json: r#"{"path":"file.rs","content":"exact content"}"#.into(),
          status: "pending".into(),
          expires_at_ms: *expiry,
        })
        .collect(),
    },
  );
}

#[test]
fn approval_refresh_cannot_redirect_a_decision_to_another_pending_request() {
  let mut app = app();
  let (actions, mut receiver) = mpsc::channel(8);
  approvals(&mut app, &[("first", i64::MAX), ("second", i64::MAX)]);
  key(&mut app, &actions, KeyCode::F(5), false);
  key(&mut app, &actions, KeyCode::Down, false);
  approvals(&mut app, &[("second", i64::MAX), ("first", i64::MAX)]);
  key(&mut app, &actions, KeyCode::Char('y'), true);
  assert!(
    matches!(receiver.try_recv(), Ok(Action::Approve { id, approved: true }) if id == "second")
  );
  update::apply(&mut app, Update::ApprovalDecided("second".into()));
  key(&mut app, &actions, KeyCode::Char('y'), true);
  assert!(
    receiver.try_recv().is_err(),
    "removed selection must not approve the next row"
  );
  key(&mut app, &actions, KeyCode::Down, false);
  approvals(&mut app, &[("first", 0)]);
  key(&mut app, &actions, KeyCode::Char('e'), true);
  assert!(
    receiver.try_recv().is_err(),
    "expired approval cannot be decided"
  );
  approvals(&mut app, &[("first", i64::MAX)]);
  key(&mut app, &actions, KeyCode::Char('e'), true);
  assert!(
    matches!(receiver.try_recv(), Ok(Action::Approve { id, approved: false }) if id == "first")
  );
}

#[test]
fn active_input_retries_keep_identity_and_steering_never_starts_another_run() {
  let mut app = app();
  app.active_run = Some("run".into());
  app.input = "please use the fixture".into();
  let (actions, mut receiver) = mpsc::channel(8);
  key(&mut app, &actions, KeyCode::Enter, false);
  let Action::SubmitInput {
    attempt: first,
    generation,
  } = receiver.try_recv().expect("queued input")
  else {
    panic!("must queue rather than start a run")
  };
  assert_eq!(generation, 7);
  assert_eq!(first.kind, QueueKind::FollowUp);
  assert_eq!(first.run, "run");
  assert_eq!(first.session, "session");
  assert!(!first.request_id.is_empty());
  key(&mut app, &actions, KeyCode::Enter, false);
  assert!(
    receiver.try_recv().is_err(),
    "in-flight input is not resubmitted"
  );
  update::apply(
    &mut app,
    Update::Notice("transport result unknown; retry explicitly".into()),
  );
  key(&mut app, &actions, KeyCode::Enter, false);
  assert!(
    matches!(receiver.try_recv(), Ok(Action::SubmitInput { attempt, .. }) if attempt == first)
  );
  assert_eq!(app.input.text(), first.prompt);
  assert!(app.interaction.history.entries("session").is_empty());
  update::apply(&mut app, Update::Notice("retry may be selected".into()));
  key(&mut app, &actions, KeyCode::Char('s'), true);
  let Action::SubmitInput { attempt: steer, .. } = receiver.try_recv().expect("steering input")
  else {
    panic!("steering must not start a run")
  };
  assert_eq!(
    steer, first,
    "unconfirmed input cannot silently change kind or identity"
  );
  update::apply(
    &mut app,
    Update::Notice("acknowledgement can now be inspected".into()),
  );
  acknowledge(&mut app, &first.prompt);
  app.input = "new explicit steering input".into();
  key(&mut app, &actions, KeyCode::Char('s'), true);
  let Action::SubmitInput { attempt: steer, .. } = receiver.try_recv().expect("fresh steering")
  else {
    panic!("steering")
  };
  assert_eq!(steer.kind, QueueKind::Steer);
  assert_ne!(steer.request_id, first.request_id);
  update::apply(
    &mut app,
    Update::Inputs {
      generation: 7,
      run: "run".into(),
      inputs: vec![v1::RunInputReceipt {
        run_id: "run".into(),
        request_id: steer.request_id,
        entry_id: "entry".into(),
        kind: v1::RunInputKind::Steer as i32,
        state: v1::RunInputState::Pending as i32,
        prompt: steer.prompt,
        prompt_truncated: false,
      }],
    },
  );
  key(&mut app, &actions, KeyCode::F(7), false);
  assert_eq!(app.router.current(), ContextPanel::Inputs);
  assert!(matches!(receiver.try_recv(), Ok(Action::ListInputs { run, .. }) if run == "run"));
  key(&mut app, &actions, KeyCode::Delete, false);
  assert!(
    matches!(receiver.try_recv(), Ok(Action::CancelInput { run, entry, .. }) if run == "run" && entry == "entry")
  );
  assert_eq!(
    app.input_receipts[0].state,
    v1::RunInputState::Pending as i32,
    "cancellation waits for daemon confirmation"
  );
}

fn event(app: &mut App, cursor: u64, value: AgentEvent) {
  update::apply(
    app,
    Update::Event {
      generation: 7,
      event: v1::Event {
        run_id: "run".into(),
        cursor,
        created_at_ms: cursor as i64,
        kind: value.kind().into(),
        payload_json: serde_json::to_string(&value).expect("typed event"),
      },
    },
  );
}

#[test]
fn explicit_phase_survives_text_and_activity_keys_only_change_presentation() {
  let mut app = app();
  app.active_run = Some("run".into());
  let (actions, mut receiver) = mpsc::channel(8);
  event(
    &mut app,
    1,
    AgentEvent::PhaseChanged {
      phase: ExecutionPhase::Thinking,
      model: Some(ModelIdentity {
        provider: "fixture".into(),
        model: "mock".into(),
        context_window: Some(4096),
      }),
    },
  );
  event(
    &mut app,
    2,
    AgentEvent::TextDelta {
      text: "this text does not announce a phase".into(),
    },
  );
  assert!(app.activity.summary().contains("模型正在思考"));
  assert!(app.activity.summary().contains("fixture / mock"));
  assert_eq!(
    app.streaming.as_ref().map(|(_, text)| text.as_str()),
    Some("this text does not announce a phase")
  );
  event(
    &mut app,
    3,
    AgentEvent::ToolStarted {
      call_id: "read".into(),
      name: "read_file".into(),
      arguments: serde_json::json!({"path":"fixture.txt"}),
    },
  );
  event(
    &mut app,
    4,
    AgentEvent::ToolFinished {
      call_id: "read".into(),
      name: "read_file".into(),
      text: "fixture content".into(),
      is_error: false,
    },
  );
  key(&mut app, &actions, KeyCode::F(6), false);
  key(&mut app, &actions, KeyCode::Down, false);
  key(&mut app, &actions, KeyCode::Enter, false);
  assert_eq!(app.router.current(), ContextPanel::Activity);
  assert!(app.activity.items()[1].expanded);
  assert_eq!(app.activity.items()[1].result, "fixture content");
  assert!(receiver.try_recv().is_err());
}

#[test]
fn steering_never_forwards_reserved_management_commands_to_the_model() {
  let mut app = app();
  app.active_run = Some("run".into());
  app.input = "/permissions".into();
  let (actions, mut receiver) = mpsc::channel(4);
  key(&mut app, &actions, KeyCode::Char('s'), true);
  assert!(receiver.try_recv().is_err());
  assert_eq!(app.input.text(), "/permissions");
  assert!(app.notice.contains("本地处理"));
}

#[test]
fn unknown_input_keeps_its_identity_after_the_original_run_has_finished() {
  let mut app = app();
  app.active_run = Some("old-run".into());
  app.input = "same uncertain input".into();
  let (actions, mut receiver) = mpsc::channel(4);
  key(&mut app, &actions, KeyCode::Char('s'), true);
  let Action::SubmitInput {
    attempt: original, ..
  } = receiver.try_recv().unwrap()
  else {
    panic!("steering")
  };
  update::apply(&mut app, Update::Notice("response lost".into()));
  update::apply(
    &mut app,
    Update::Runs {
      session: "session".into(),
      generation: 7,
      runs: vec![v1::Run {
        id: "old-run".into(),
        session_id: "session".into(),
        status: "completed".into(),
        ..Default::default()
      }],
    },
  );
  assert_eq!(app.active_run, None);
  key(&mut app, &actions, KeyCode::Enter, false);
  let Action::SubmitInput { attempt: retry, .. } = receiver.try_recv().unwrap() else {
    panic!("must never start a new run for an unconfirmed input")
  };
  assert_eq!(retry, original);
  assert_eq!(app.input.text(), "same uncertain input");
  assert!(app.interaction.history.entries("session").is_empty());
  acknowledge(&mut app, "same uncertain input");
  assert!(app.input.is_empty());
  assert_eq!(app.interaction.history.entries("session").len(), 1);
}
