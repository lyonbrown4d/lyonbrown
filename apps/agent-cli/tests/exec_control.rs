mod exec_support;
#[allow(dead_code)]
mod support;

use agent_test_support::{MockModelServer, ScriptedReply};
use exec_support::{command, finish, invoke, records};
use support::node::Node;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deadline_requests_cancellation_and_waits_for_persisted_state() {
  let model = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let node = Node::start(&model).await;
  let output = invoke(
    &node,
    &["Wait", "--timeout", "3", "--output-format", "jsonl"],
    b"",
  )
  .await;
  assert_eq!(
    output.status.code(),
    Some(124),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  let lines = records(&output);
  let result = lines.last().unwrap();
  assert_eq!(result["type"], "result");
  assert_eq!(result["reason"], "timeout");
  assert_eq!(result["status"], "cancelled");
  let id = result["run_id"].as_str().unwrap();
  assert_eq!(
    node.daemon.store.run(id).unwrap().unwrap().status,
    "cancelled"
  );
  assert!(
    node
      .daemon
      .store
      .events(id, 0, 1000)
      .unwrap()
      .iter()
      .any(|event| event.kind == "run_cancelled")
  );
  assert_eq!(model.requests().await.len(), 1);
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deadline_can_exit_while_stdin_pipe_remains_open() {
  let model = MockModelServer::start(Vec::new()).await.unwrap();
  let node = Node::start(&model).await;
  let mut child = command(&node, &["-", "--timeout", "1", "--output-format", "jsonl"])
    .spawn()
    .unwrap();
  let held_open = child.stdin.take().unwrap();
  let output = finish(child).await;
  drop(held_open);
  assert_eq!(output.status.code(), Some(124));
  let lines = records(&output);
  assert_eq!(lines.last().unwrap()["type"], "stopped");
  assert_eq!(lines.last().unwrap()["cancellation_confirmed"], false);
  assert!(node.client.list_sessions().await.unwrap().is_empty());
  assert!(model.requests().await.is_empty());
  node.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupt_and_terminate_cancel_only_the_acknowledged_run() {
  use tokio::io::{AsyncBufReadExt as _, BufReader};
  for signal in ["-INT", "-TERM"] {
    let model = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let node = Node::start(&model).await;
    let mut child = command(&node, &["Wait", "--output-format", "jsonl"])
      .spawn()
      .unwrap();
    drop(child.stdin.take());
    let pid = child.id().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let id = tokio::time::timeout(std::time::Duration::from_secs(12), async {
      loop {
        let line = lines.next_line().await.unwrap().expect("accepted record");
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        if value["type"] == "accepted" {
          break value["run_id"].as_str().unwrap().to_owned();
        }
      }
    })
    .await
    .unwrap();
    model.wait_for_requests(1).await;
    assert!(
      std::process::Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .unwrap()
        .success()
    );
    let last = tokio::time::timeout(std::time::Duration::from_secs(12), async {
      let mut last = None;
      while let Some(line) = lines.next_line().await.unwrap() {
        last = Some(serde_json::from_str::<serde_json::Value>(&line).unwrap());
      }
      last.unwrap()
    })
    .await
    .unwrap();
    let output = finish(child).await;
    assert_eq!(
      output.status.code(),
      Some(130),
      "{}",
      String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(last["reason"], "interrupted");
    assert_eq!(last["status"], "cancelled");
    assert_eq!(
      node.daemon.store.run(&id).unwrap().unwrap().status,
      "cancelled"
    );
    assert_eq!(model.requests().await.len(), 1);
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transport_loss_does_not_replay_or_cancel_daemon_work() {
  use tokio::io::{AsyncBufReadExt as _, BufReader};
  let model = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let node = Node::start(&model).await;
  let mut child = command(&node, &["Wait", "--output-format", "jsonl"])
    .spawn()
    .unwrap();
  drop(child.stdin.take());
  let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
  let id = loop {
    let line = tokio::time::timeout(std::time::Duration::from_secs(12), lines.next_line())
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    if value["type"] == "accepted" {
      break value["run_id"].as_str().unwrap().to_owned();
    }
  };
  model.wait_for_requests(1).await;
  node.disconnect().await;
  let output = finish(child).await;
  assert_eq!(output.status.code(), Some(1));
  let mut last = None;
  while let Some(line) = lines.next_line().await.unwrap() {
    last = Some(serde_json::from_str::<serde_json::Value>(&line).unwrap());
  }
  let last = last.unwrap();
  assert_eq!(last["type"], "error");
  assert_eq!(last["phase"], "follow");
  assert_eq!(
    node.daemon.store.run(&id).unwrap().unwrap().status,
    "running"
  );
  assert_eq!(model.requests().await.len(), 1);
  node.client.cancel_run(&id).await.unwrap();
  node.status(&id, "cancelled").await;
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uncertain_run_is_reported_without_replaying_it() {
  let model = MockModelServer::start(Vec::new()).await.unwrap();
  let node = Node::start(&model).await;
  let session = node
    .client
    .create_session("Interrupted", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let (run, _) = node
    .daemon
    .store
    .accept_run(&session.id, session.owner_epoch, "old prompt", "stable")
    .unwrap();
  let before = node
    .daemon
    .store
    .update_run_owned(
      &run.id,
      session.owner_epoch,
      agent_core::RunStatus::Uncertain,
      "",
      "unknown effects",
    )
    .unwrap();
  let output = invoke(
    &node,
    &[
      "old prompt",
      "--session",
      &session.id,
      "--idempotency-key",
      "stable",
      "--output-format",
      "jsonl",
    ],
    b"",
  )
  .await;
  assert_eq!(output.status.code(), Some(4));
  assert_eq!(records(&output).last().unwrap()["status"], "uncertain");
  assert_eq!(node.daemon.store.run(&run.id).unwrap(), Some(before));
  assert!(model.requests().await.is_empty());
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unread_jsonl_pipe_cannot_block_deadline_or_runtime_shutdown() {
  let model = MockModelServer::start(vec![agent_test_support::text_reply(
    agent_test_support::StandardProtocol::ChatCompletions,
    &"x".repeat(131_072),
  )])
  .await
  .unwrap();
  let node = Node::start(&model).await;
  let mut child = command(
    &node,
    &["Reply", "--timeout", "3", "--output-format", "jsonl"],
  )
  .spawn()
  .unwrap();
  drop(child.stdin.take());
  // Deliberately retain stdout without reading; wait_with_output would drain it.
  let status = tokio::time::timeout(std::time::Duration::from_secs(12), child.wait())
    .await
    .expect("a blocked pipe must not hold the CLI alive")
    .unwrap();
  assert_eq!(status.code(), Some(124));
  let session = node.client.list_sessions().await.unwrap().pop().unwrap();
  let runs = node.client.list_runs(&session.id).await.unwrap();
  assert_eq!(runs.len(), 1);
  assert!(matches!(runs[0].status.as_str(), "completed" | "cancelled"));
  assert_eq!(model.requests().await.len(), 1);
  node.close().await;
}
