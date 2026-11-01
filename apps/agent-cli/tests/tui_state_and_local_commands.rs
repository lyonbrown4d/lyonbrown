use agent_cli::{
  backend::{Action, Update},
  input,
  local_state::{ClientState, PersistedPanel},
  model::{App, ContextPanel, Session},
  update,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

fn session(id: &str) -> Session {
  Session {
    id: id.into(),
    title: format!("Session {id}"),
  }
}

#[test]
fn slash_palette_filters_completes_and_keeps_unknown_commands_local() {
  let mut app = App::default();
  let (actions, mut receiver) = mpsc::channel(2);
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
    &actions,
  )
  .expect("open slash palette");
  assert_eq!(app.panel, ContextPanel::Commands);
  assert_eq!(app.input.text(), "/");

  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)),
    &actions,
  )
  .expect("filter slash commands");
  assert_eq!(app.input.text(), "/t");
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
    &actions,
  )
  .expect("complete selected command");
  assert_eq!(app.input.text(), "/task ");
  assert_eq!(app.panel, ContextPanel::Closed);

  app.input = "/not-a-command run this".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("reject unknown slash command locally");
  assert!(app.notice.contains("未知命令"));
  assert!(
    receiver.try_recv().is_err(),
    "unknown slash text is never submitted"
  );
}

#[test]
fn p0_resource_commands_dispatch_only_metadata_and_open_sessions_panel() {
  let mut app = App {
    sessions: vec![session("one"), session("two")],
    ..App::default()
  };
  let (actions, mut receiver) = mpsc::channel(8);

  app.input = "/status".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("dispatch status query");
  assert!(matches!(
    receiver.try_recv(),
    Ok(Action::Resource {
      command: agent_cli::resource_command::ResourceCommand::Status,
      ..
    })
  ));

  app.input = "/git".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("dispatch Git status query");
  assert!(matches!(
    receiver.try_recv(),
    Ok(Action::Resource {
      command: agent_cli::resource_command::ResourceCommand::Git,
      ..
    })
  ));

  app.input = "/skills list workspace".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("dispatch Skill listing in workspace scope");
  assert!(matches!(
    receiver.try_recv(),
    Ok(Action::Resource {
      command: agent_cli::resource_command::ResourceCommand::SkillsList { scope },
      ..
    }) if scope == "workspace"
  ));

  app.input = "/providers upload openai OPENAI_API_KEY".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("dispatch explicit provider upload by allowlisted name");
  let dispatched = receiver.try_recv().expect("provider action");
  assert!(matches!(
    &dispatched,
    Action::Resource {
      command: agent_cli::resource_command::ResourceCommand::ProviderUpload {
        provider,
        environment,
      },
      ..
    } if provider == "openai" && environment == "OPENAI_API_KEY"
  ));
  if let Action::Resource { command, .. } = dispatched {
    assert!(!format!("{command:?}").contains("test-secret"));
  } else {
    panic!("provider upload must dispatch a resource command");
  }

  app.input = "/sessions".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("open session browser");
  assert_eq!(app.panel, ContextPanel::Sessions);
  assert!(receiver.try_recv().is_err());
}

#[test]
fn saved_drafts_restore_known_sessions_and_preserve_orphans_for_explicit_recovery() {
  let mut app = App {
    sessions: vec![session("known-a"), session("known-b")],
    ..App::default()
  };
  update::apply(
    &mut app,
    Update::LocalState {
      node_id: Some("node-1".into()),
      state: Some(ClientState {
        selected_session: Some("known-b".into()),
        drafts: [
          ("known-a".into(), "draft A".into()),
          ("known-b".into(), "draft B".into()),
          ("removed-session".into(), "must not leak".into()),
        ]
        .into(),
        unbound_draft: "unbound".into(),
        panel: PersistedPanel::Tasks,
        transcript_scroll: 13,
        context_scroll: 7,
      }),
      warning: None,
      preserve: false,
    },
  );
  assert_eq!(
    app.selected_session().map(|value| value.id.as_str()),
    Some("known-b")
  );
  assert_eq!(app.input.text(), "draft B");
  assert_eq!(app.drafts.len(), 3);
  assert_eq!(
    app.drafts.get("removed-session").map(String::as_str),
    Some("must not leak")
  );
  assert_eq!(app.panel, ContextPanel::Tasks);
  assert_eq!((app.scroll, app.computer_scroll), (13, 7));

  app.next_session();
  assert_eq!(app.input.text(), "draft A");
  app.input = "edited A".into();
  let state = app.client_state();
  assert_eq!(
    state.drafts.get("known-a").map(String::as_str),
    Some("edited A")
  );
  assert_eq!(
    state.drafts.get("known-b").map(String::as_str),
    Some("draft B")
  );
  assert_eq!(
    state.drafts.get("removed-session").map(String::as_str),
    Some("must not leak")
  );

  let (actions, mut receiver) = mpsc::channel(2);
  app.input = "/draft recover removed-session".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("recover orphan draft");
  assert_eq!(app.input.text(), "must not leak");
  assert!(app.draft_detail.contains("removed-session"));
  assert!(
    receiver.try_recv().is_err(),
    "recovery does not submit or call daemon"
  );
  app.input = "edited orphan retained without binding".into();
  app.save_current_draft();
  app.next_session();
  app.input = "/draft recover unbound".into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("recover edited unbound draft");
  assert_eq!(app.input.text(), "edited orphan retained without binding");
  assert!(receiver.try_recv().is_err());
}

#[test]
fn acknowledged_submission_clears_only_the_matching_session_draft() {
  let mut app = App {
    sessions: vec![session("one"), session("two")],
    ..App::default()
  };
  app.input = "same text".into();
  app.save_current_draft();
  app.next_session();
  app.input = "same text".into();
  app.save_current_draft();
  app.next_session();
  app.input = "newer edit".into();
  app.save_current_draft();

  update::apply(
    &mut app,
    Update::Submitted {
      session: "one".into(),
      prompt: "same text".into(),
      generation: 0,
    },
  );
  assert_eq!(app.input.text(), "newer edit");
  assert_eq!(
    app.drafts.get("one").map(String::as_str),
    Some("newer edit")
  );
  assert_eq!(app.drafts.get("two").map(String::as_str), Some("same text"));
  update::apply(
    &mut app,
    Update::Submitted {
      session: "two".into(),
      prompt: "same text".into(),
      generation: 0,
    },
  );
  assert!(
    !app.drafts.contains_key("two"),
    "a late acknowledgement clears only its matching saved draft"
  );
}

#[test]
fn stale_session_generation_cannot_replace_task_or_run_view() {
  let mut app = App {
    generation: 9,
    sessions: vec![session("one")],
    task_rows: vec![serde_json::json!({"status":"current"})],
    ..App::default()
  };
  update::apply(
    &mut app,
    Update::TaskResult {
      session: Some("one".into()),
      generation: 8,
      source: None,
      append: false,
      value: serde_json::json!({"tasks":[],"next_task_id":""}),
    },
  );
  assert_eq!(app.task_rows, vec![serde_json::json!({"status":"current"})]);
  update::apply(
    &mut app,
    Update::Runs {
      session: "one".into(),
      generation: 8,
      runs: vec![agent_protocol::v1::Run {
        id: "stale".into(),
        ..Default::default()
      }],
    },
  );
  assert!(app.messages.is_empty());
}

#[tokio::test]
async fn malformed_task_command_is_local_and_never_becomes_a_model_prompt() {
  let (actions, mut receiver) = mpsc::channel(4);
  let mut app = App {
    connection: agent_cli::model::Connection::Connected,
    sessions: vec![session("one")],
    ..App::default()
  };
  app.input = "/task dispatch --node-id missing".into();
  let closed = input::handle(
    &mut app,
    Event::Key(KeyEvent {
      code: KeyCode::Enter,
      modifiers: KeyModifiers::NONE,
      kind: KeyEventKind::Press,
      state: crossterm::event::KeyEventState::NONE,
    }),
    &actions,
  )
  .expect("handle local command");
  assert!(!closed);
  assert!(app.task_detail.contains("error") || app.task_detail.contains("required"));
  assert!(matches!(
    receiver.try_recv(),
    Err(mpsc::error::TryRecvError::Empty)
  ));
  assert!(!matches!(receiver.try_recv(), Ok(Action::Submit { .. })));
}

#[tokio::test]
async fn typed_task_command_uses_local_adapter_and_clears_only_on_result() {
  let (actions, mut receiver) = mpsc::channel(4);
  let mut app = App {
    connection: agent_cli::model::Connection::Connected,
    sessions: vec![session("one")],
    ..App::default()
  };
  let source = "/task list --session one";
  app.input = source.into();
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("route task command");
  assert_eq!(
    app.input.text(),
    source,
    "keep the command until daemon response"
  );
  let action = receiver.try_recv().expect("task adapter action");
  assert!(matches!(action, Action::Task { source: Some(ref command), .. } if command == source));
  assert!(
    receiver.try_recv().is_err(),
    "task command is not a model submission"
  );

  update::apply(
    &mut app,
    Update::TaskResult {
      session: Some("one".into()),
      generation: 0,
      source: Some(source.into()),
      append: false,
      value: serde_json::json!({"tasks": [], "next_task_id": ""}),
    },
  );
  assert!(app.input.is_empty());
}

#[tokio::test]
async fn task_panel_gets_selected_persistent_task_and_requests_next_page() {
  let (actions, mut receiver) = mpsc::channel(4);
  let mut app = App {
    connection: agent_cli::model::Connection::Connected,
    state_loaded: true,
    sessions: vec![session("one")],
    panel: ContextPanel::Tasks,
    generation: 3,
    ..App::default()
  };
  update::apply(
    &mut app,
    Update::TaskResult {
      session: Some("one".into()),
      generation: 3,
      source: None,
      append: false,
      value: serde_json::json!({
        "tasks": [{"envelope":{"task_id":"task-1"},"status":"succeeded","attempt_status":"succeeded"}],
        "next_task_id":"task-cursor"
      }),
    },
  );
  assert_eq!(app.task_rows.len(), 1);
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &actions,
  )
  .expect("get selected task");
  assert!(matches!(
    receiver.try_recv(),
    Ok(Action::Task { command: agent_cli::task_command::TaskCommand::Get { task_id }, .. }) if task_id == "task-1"
  ));
  update::apply(
    &mut app,
    Update::TaskResult {
      session: Some("one".into()),
      generation: 3,
      source: None,
      append: false,
      value: serde_json::json!({"status":"succeeded","result":"review required"}),
    },
  );
  assert!(app.task_detail.contains("succeeded"));
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
    &actions,
  )
  .expect("request next task page");
  assert!(matches!(
    receiver.try_recv(),
    Ok(Action::Task { command: agent_cli::task_command::TaskCommand::List { after: Some(after), .. }, .. }) if after == "task-cursor"
  ));
  update::apply(
    &mut app,
    Update::TaskResult {
      session: Some("one".into()),
      generation: 3,
      source: None,
      append: true,
      value: serde_json::json!({
        "tasks": [{"envelope":{"task_id":"task-2"},"status":"uncertain","attempt_status":"unknown"}],
        "next_task_id":""
      }),
    },
  );
  assert_eq!(
    app.task_rows.len(),
    2,
    "next page appends to the current list"
  );
}

#[tokio::test]
async fn typed_remote_task_commands_preserve_target_and_explicit_acceptance() {
  let (actions, mut receiver) = mpsc::channel(8);
  let mut app = App {
    connection: agent_cli::model::Connection::Connected,
    state_loaded: true,
    sessions: vec![session("one")],
    ..App::default()
  };
  let commands = [
    "/task dispatch --node-id worker --node-endpoint https://worker.test --session one --parent-run run-1 --task-id 01900000-0000-7000-8000-000000000001 --attempt-id 01900000-0000-7000-8000-000000000002 --idempotency-key dispatch-1 --instructions 'review this change' --capability workspace.read",
    "/task cancel task-1 --node-id worker --node-endpoint https://worker.test",
    "/task reconcile task-1 --node-id worker --node-endpoint https://worker.test",
    "/task accept task-1 --node-id worker --node-endpoint https://worker.test --accepted false",
  ];
  for (index, command) in commands.into_iter().enumerate() {
    app.input = command.into();
    input::handle(
      &mut app,
      Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
      &actions,
    )
    .expect("parse remote task command");
    let action = receiver.try_recv().expect("task adapter action");
    match (index, action) {
      (
        0,
        Action::Task {
          command: agent_cli::task_command::TaskCommand::Dispatch(args),
          ..
        },
      ) => {
        assert_eq!(args.task_id, "01900000-0000-7000-8000-000000000001");
        assert_eq!(args.attempt_id, "01900000-0000-7000-8000-000000000002");
        assert_eq!(args.idempotency_key, "dispatch-1");
      }
      (
        1,
        Action::Task {
          command: agent_cli::task_command::TaskCommand::Cancel(args),
          ..
        },
      ) => {
        assert_eq!(args.task_id, "task-1");
        assert_eq!(args.target.node_id, "worker");
      }
      (
        2,
        Action::Task {
          command: agent_cli::task_command::TaskCommand::Reconcile(args),
          ..
        },
      ) => {
        assert_eq!(args.task_id, "task-1");
        assert_eq!(args.target.node_endpoint, "https://worker.test");
      }
      (
        3,
        Action::Task {
          command: agent_cli::task_command::TaskCommand::Accept { task, accepted },
          ..
        },
      ) => {
        assert_eq!(task.task_id, "task-1");
        assert!(!accepted);
      }
      (_, _) => panic!("unexpected task action variant"),
    }
  }
  assert!(receiver.try_recv().is_err());
}
