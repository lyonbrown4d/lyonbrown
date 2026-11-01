#[allow(dead_code)]
mod support;

use agent_cli::{config::Config, run_command::RunCommand, run_management, task_command::Command};
use agent_core::RunStatus;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use clap::Parser;
use support::{node::Node, pty::Terminal};

#[test]
fn recovery_requires_explicit_identity_note_and_new_input() {
  assert!(
    Config::try_parse_from([
      "agent-cli",
      "run",
      "continue",
      "run-1",
      "--idempotency-key",
      "key",
      "--note",
      "checked"
    ])
    .is_err()
  );
  assert!(
    Config::try_parse_from([
      "agent-cli",
      "run",
      "abandon",
      "run-1",
      "--idempotency-key",
      "key"
    ])
    .is_err()
  );
}

#[test]
fn recovery_commands_stay_local_and_stale_results_preserve_the_current_draft() {
  use agent_cli::{
    backend::{Action, Update},
    input,
    model::{App, Session},
    update,
  };
  use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
  let mut app = App {
    sessions: vec![Session {
      id: "session".into(),
      title: "Current".into(),
    }],
    input: "/run inspect interrupted".into(),
    generation: 3,
    ..App::default()
  };
  let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
  input::handle(
    &mut app,
    Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    &sender,
  )
  .unwrap();
  assert!(matches!(
    receiver.try_recv().unwrap(),
    Action::Run {
      command: RunCommand::Inspect { .. },
      ..
    }
  ));
  app.input = "keep this draft".into();
  let value = serde_json::json!({"session":{"id":"new","title":"Recovery"},"run":{"id":"new-run"}});
  update::apply(
    &mut app,
    Update::RecoveryResult {
      session: Some("session".into()),
      generation: 2,
      source: "keep this draft".into(),
      value: value.clone(),
    },
  );
  assert_eq!(app.sessions.len(), 1);
  assert_eq!(app.input.text(), "keep this draft");
  update::apply(
    &mut app,
    Update::RecoveryResult {
      session: Some("session".into()),
      generation: 3,
      source: "/run inspect interrupted".into(),
      value,
    },
  );
  assert_eq!(app.sessions.len(), 2);
  assert_eq!(app.selected_session().unwrap().id, "session");
  assert_eq!(app.input.text(), "keep this draft");
  update::apply(
    &mut app,
    Update::RecoveryResult {
      session: Some("session".into()),
      generation: 3,
      source: "/run inspect lost-response".into(),
      value: serde_json::json!({"resolution":{"session":{"id":"recovered-from-inspection","title":"Recovered"}}}),
    },
  );
  assert!(
    app
      .sessions
      .iter()
      .any(|session| session.id == "recovered-from-inspection")
  );
  assert_eq!(app.input.text(), "keep this draft");
  app.task_detail = "note with \u{202e}direction override".into();
  let theme = agent_cli::theme::Theme::resolve(agent_cli::config::ThemeChoice::Dark, None);
  let view = agent_cli::view_model::ViewModel::new(&app, theme);
  assert!(!view.context.unwrap().1.to_string().contains('\u{202e}'));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tui_inspects_and_continues_in_a_fresh_session_without_replaying_the_original() {
  let mock = MockModelServer::start(vec![text_reply(
    StandardProtocol::ChatCompletions,
    "fresh result",
  )])
  .await
  .unwrap();
  let node = Node::start(&mock).await;
  let session = node
    .client
    .create_session("Interrupted", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let (original, _) = node
    .daemon
    .store
    .accept_run(
      &session.id,
      session.owner_epoch,
      "original must never replay",
      "original-key",
    )
    .unwrap();
  node
    .daemon
    .store
    .update_run(&original.id, RunStatus::Running, "", "")
    .unwrap();
  node.daemon.recover().await.unwrap();
  assert_eq!(
    node.client.get_run(&original.id).await.unwrap().status,
    "uncertain"
  );
  let mut terminal = Terminal::start(&node);
  terminal.screen("Connected").await;
  terminal.screen("/run inspect").await;
  terminal.paste(&format!("/run inspect {}", original.id));
  terminal.send("\r");
  terminal.screen("fresh_session_required").await;
  assert!(
    mock.requests().await.is_empty(),
    "inspection never runs the model"
  );
  let command = format!(
    "/run continue {} --idempotency-key tui-recovery --note 'checked workspace and external effects' --prompt 'fresh input after verification'",
    original.id
  );
  terminal.paste(&command);
  terminal.send("\r");
  terminal.screen("continue_new_session").await;
  terminal.screen_without("/run continue").await;
  let resolution = node
    .daemon
    .store
    .run_recovery(&original.id)
    .unwrap()
    .unwrap();
  let new_id = resolution.replacement_run_id.as_ref().unwrap();
  node.status(new_id, "completed").await;
  let config = Config::try_parse_from([
    "agent-cli",
    "run",
    "continue",
    &original.id,
    "--idempotency-key",
    "tui-recovery",
    "--note",
    "checked workspace and external effects",
    "--prompt",
    "fresh input after verification",
  ])
  .unwrap();
  let Some(Command::Run { command }) = config.command else {
    panic!("run command")
  };
  assert!(matches!(&command, RunCommand::Continue { .. }));
  let repeated = run_management::execute(&node.client, command)
    .await
    .unwrap();
  assert_eq!(repeated["run"]["id"], *new_id);
  let requests = mock.requests().await;
  assert_eq!(
    requests.len(),
    1,
    "recovery retry must not run a second model call"
  );
  let body = requests[0].body.to_string();
  assert!(body.contains("fresh input after verification"));
  assert!(!body.contains("original must never replay"));
  assert_eq!(node.client.list_runs(&session.id).await.unwrap().len(), 1);
  assert_eq!(
    node.client.get_run(&original.id).await.unwrap().status,
    "uncertain"
  );
  terminal.quit().await;
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inspection_exposes_blocking_tasks_without_losing_empty_page_cursors() {
  let mock = MockModelServer::start(Vec::new()).await.unwrap();
  let node = Node::start(&mock).await;
  let session = node
    .client
    .create_session(
      "Task reconciliation",
      node.workspace.path().to_str().unwrap(),
    )
    .await
    .unwrap();
  let (run, _) = node
    .daemon
    .store
    .accept_run(&session.id, 1, "interrupted", "original")
    .unwrap();
  node
    .daemon
    .store
    .update_run_owned(&run.id, 1, RunStatus::Uncertain, "", "unknown effects")
    .unwrap();
  for index in 0..101 {
    let task = agent_core::TaskEnvelope {
      task_id: format!("task-{index:03}"),
      parent_run_id: run.id.clone(),
      session_id: session.id.clone(),
      owner_node_id: session.owner_node_id.clone(),
      owner_epoch: 1,
      attempt_id: format!("attempt-{index}"),
      instructions: "synthetic task".into(),
      capabilities: vec!["workspace.read".into()],
      workspace: session.workspace.clone(),
      idempotency_key: format!("task-key-{index}"),
      deadline_ms: agent_core::now_ms() + 60_000,
    };
    node.daemon.store.accept_task(task.clone()).unwrap();
    if index < 100 {
      node
        .daemon
        .store
        .mark_task_cancelled(&task.task_id, &task.attempt_id, 1)
        .unwrap();
    }
  }
  let value = run_management::execute(
    &node.client,
    RunCommand::Inspect {
      run_id: run.id.clone(),
    },
  )
  .await
  .unwrap();
  assert!(value["allowed_actions"].as_array().unwrap().is_empty());
  assert!(value["unresolved_tasks"].as_array().unwrap().is_empty());
  let cursor = value["next_task_id"].as_str().unwrap();
  assert!(
    !cursor.is_empty(),
    "an empty filtered page is not the end of the session"
  );
  let page = node
    .client
    .recovery_tasks(&session.id, cursor)
    .await
    .unwrap();
  assert_eq!(page.tasks.len(), 1);
  assert_eq!(page.tasks[0].envelope.as_ref().unwrap().task_id, "task-100");
  assert!(page.next_task_id.is_empty());
  assert!(mock.requests().await.is_empty());
  node.close().await;
}
