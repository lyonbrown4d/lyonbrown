#[allow(dead_code)]
mod support;

#[path = "../../agentd/tests/peer_support/mod.rs"]
#[allow(dead_code)]
mod peer_support;
#[path = "../../agentd/tests/remote_support/mod.rs"]
#[allow(dead_code)]
mod remote_support;

use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use serde_json::json;
use std::time::Duration;
use support::{node::Node, pty::Terminal};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tui_task_commands_drive_one_paired_worker_attempt_to_durable_acceptance() {
  let protocol = StandardProtocol::ChatCompletions;
  let mock = MockModelServer::start(vec![text_reply(protocol, "remote result")])
    .await
    .unwrap();
  let remote = remote_support::Remote::open(&mock, protocol).await;
  let envelope = remote.pair.envelope(vec!["workspace.read".into()]);
  let task_id = uuid::Uuid::parse_str(&envelope.task_id)
    .expect("fixture task ID")
    .to_string();
  let attempt_id = uuid::Uuid::parse_str(&envelope.attempt_id)
    .expect("fixture attempt ID")
    .to_string();
  let baseline_master_runs = remote
    .pair
    .master
    .store
    .runs(&remote.pair.session.id)
    .expect("existing parent run");
  assert_eq!(baseline_master_runs.len(), 1);
  assert_eq!(baseline_master_runs[0].id, envelope.parent_run_id);
  assert_eq!(baseline_master_runs[0].status, "running");

  let target = remote.target();
  let data = tempfile::tempdir().unwrap();
  let workspace = tempfile::tempdir().unwrap();
  let mut terminal = Terminal::start_with(
    &remote.endpoint,
    remote.pair.master.auth_token(),
    data.path(),
    workspace.path(),
  );
  terminal.screen("Connected").await;
  let dispatch = format!(
    "/task dispatch --node-id {} --node-endpoint {} --session {} --parent-run {} --task-id {} --attempt-id {} --idempotency-key {} --instructions 'Read master-only.txt and summarize it.' --capability workspace.read",
    target.id,
    target.endpoint,
    remote.pair.session.id,
    envelope.parent_run_id,
    task_id,
    attempt_id,
    envelope.idempotency_key,
  );
  terminal.paste(&dispatch);
  terminal.send("\r");
  // The terminal wraps UUIDs at narrow PTY widths, so assert a stable detail marker.
  terminal.screen("envelope").await;
  wait_store_task(&remote, &task_id, |task| {
    matches!(task.status.as_str(), "running" | "awaiting_acceptance")
  })
  .await;
  tokio::time::timeout(Duration::from_secs(10), async {
    loop {
      if remote
        .pair
        .worker
        .store
        .task(&task_id)
        .ok()
        .flatten()
        .is_some_and(|task| task.attempt_status == "succeeded")
      {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("Worker mock attempt completes");

  terminal.paste(&format!("/task get {task_id}"));
  terminal.screen("/task get").await;
  terminal.send("\r");
  terminal.screen_without("/task get").await;

  terminal.paste(&format!(
    "/task reconcile {task_id} --node-id {} --node-endpoint {}",
    target.id, target.endpoint
  ));
  terminal.send("\r");
  wait_store_task(&remote, &task_id, |task| {
    task.status == "awaiting_acceptance"
  })
  .await;
  terminal.screen_without("/task reconcile").await;

  terminal.paste(&format!(
    "/task accept {task_id} --node-id {} --node-endpoint {} --accepted true",
    target.id, target.endpoint
  ));
  terminal.send("\r");
  wait_store_task(&remote, &task_id, |task| task.status == "completed").await;
  terminal.screen_without("/task accept").await;

  assert_eq!(
    remote
      .pair
      .worker
      .store
      .task(&task_id)
      .expect("Worker task read")
      .expect("Worker task exists")
      .status,
    "completed"
  );
  assert_eq!(
    remote.pair.worker_runs(&envelope).len(),
    1,
    "one Worker run was persisted"
  );
  assert_eq!(
    remote
      .pair
      .master
      .store
      .runs(&remote.pair.session.id)
      .expect("Master runs after task")
      .len(),
    baseline_master_runs.len(),
    "task management does not create another Master model run"
  );
  assert_eq!(
    mock.requests().await.len(),
    1,
    "the Worker executes exactly once"
  );
  terminal.quit().await;
  remote.close().await;
}

async fn wait_store_task(
  remote: &remote_support::Remote,
  task_id: &str,
  ready: impl Fn(&agent_core::Task) -> bool,
) {
  tokio::time::timeout(Duration::from_secs(10), async {
    loop {
      if remote
        .pair
        .master
        .store
        .task(task_id)
        .ok()
        .flatten()
        .is_some_and(|task| ready(&task))
      {
        return;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("Master task reaches expected durable status");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn task_management_cli_uses_authenticated_daemon_without_entering_terminal() {
  let model = MockModelServer::start(vec![]).await.unwrap();
  let node = Node::start(&model).await;
  let session = node
    .client
    .create_session("task listing", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let executable = env!("CARGO_BIN_EXE_agent-cli");
  let output = tokio::process::Command::new(executable)
    .args([
      "--endpoint",
      &node.endpoint(),
      "--token",
      node.daemon.auth_token(),
      "task",
      "list",
      "--session",
      &session.id,
      "--page-size",
      "1",
    ])
    .output()
    .await
    .unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
  assert_eq!(result["tasks"], json!([]));
  assert_eq!(result["next_task_id"], "");
  let denied = tokio::process::Command::new(executable)
    .args([
      "--endpoint",
      &node.endpoint(),
      "--token",
      "invalid-credential",
      "task",
      "list",
      "--session",
      &session.id,
    ])
    .output()
    .await
    .unwrap();
  assert!(!denied.status.success());
  assert!(denied.stdout.is_empty());
  assert!(
    node
      .client
      .list_tasks(&session.id, "", 100)
      .await
      .unwrap()
      .tasks
      .is_empty()
  );
  assert!(model.requests().await.is_empty());
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tui_draft_and_task_panel_restore_after_exit_without_replaying_local_command() {
  let model = MockModelServer::start(vec![]).await.unwrap();
  let node = Node::start(&model).await;
  let session = node
    .client
    .create_session("restored draft", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();

  let mut terminal = Terminal::start(&node);
  terminal.screen("Connected").await;
  terminal.send("\x1b[13~"); // F3 task panel.
  terminal.screen("持久任务").await;
  terminal.paste("/task dispatch --node-id missing");
  terminal.send("\r");
  terminal.screen("required").await;
  assert!(node.client.list_runs(&session.id).await.unwrap().is_empty());
  assert!(model.requests().await.is_empty());

  terminal.paste("keep this draft across restart");
  terminal.screen("keep this draft across restart").await;
  terminal.quit().await;

  let mut restarted = Terminal::start(&node);
  restarted.screen("Connected").await;
  restarted.screen("keep this draft across restart").await;
  restarted.screen("持久任务").await;
  assert!(node.client.list_runs(&session.id).await.unwrap().is_empty());
  assert!(model.requests().await.is_empty());
  restarted.quit().await;
  node.close().await;
}
