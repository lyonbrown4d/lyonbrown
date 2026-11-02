mod exec_support;
#[allow(dead_code)]
mod support;

use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply,
};
use exec_support::{invoke, records};
use serde_json::json;
use support::node::Node;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_jsonl_persisted_events_and_explicit_idempotent_reuse() {
  let model = MockModelServer::start(vec![text_reply(
    StandardProtocol::ChatCompletions,
    "done\n第二行",
  )])
  .await
  .unwrap();
  let node = Node::start(&model).await;
  let output = invoke(
    &node,
    &[
      "Summarize",
      "--output-format",
      "jsonl",
      "--idempotency-key",
      "exec-stable",
    ],
    b"input context",
  )
  .await;
  assert_eq!(
    output.status.code(),
    Some(0),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert!(!String::from_utf8_lossy(&output.stdout).contains(node.daemon.auth_token()));
  assert!(!String::from_utf8_lossy(&output.stderr).contains(node.daemon.auth_token()));
  let lines = records(&output);
  assert!(lines.iter().all(|line| line["schema_version"] == 1));
  assert_eq!(lines[0]["type"], "submission");
  assert_eq!(lines[1]["type"], "accepted");
  let result = lines.last().unwrap();
  assert_eq!(result["type"], "result");
  assert_eq!(result["status"], "completed");
  assert_eq!(result["result"], "done\n第二行");
  let id = result["run_id"].as_str().unwrap();
  let session = result["session_id"].as_str().unwrap();
  let run = node.daemon.store.run(id).unwrap().unwrap();
  assert_eq!(run.status, "completed");
  assert_eq!(run.prompt, "Summarize\n\ninput context");
  let events = node.daemon.store.events(id, 0, 1000).unwrap();
  let emitted: Vec<_> = lines
    .iter()
    .filter(|line| line["type"] == "event")
    .collect();
  assert_eq!(emitted.len(), events.len());
  for (line, event) in emitted.iter().zip(&events) {
    assert_eq!(line["cursor"], event.cursor);
    assert_eq!(line["kind"], event.kind);
    assert_eq!(
      line["payload"],
      serde_json::from_str::<serde_json::Value>(&event.payload_json).unwrap()
    );
  }
  let outbox = node.daemon.store.pending_outbox(1000).unwrap();
  assert!(events.iter().all(|event| {
    outbox
      .iter()
      .any(|entry| entry.run_id == id && entry.cursor == event.cursor)
  }));
  // Existing sessions use their current fence, even after ownership advances.
  let old = node.daemon.store.session(session).unwrap().unwrap();
  let changed = node
    .daemon
    .store
    .change_owner(session, old.owner_epoch, &old.owner_node_id)
    .unwrap();
  assert_eq!(changed.owner_epoch, 2);
  let reused = invoke(
    &node,
    &[
      "Summarize",
      "--session",
      session,
      "--idempotency-key",
      "exec-stable",
    ],
    b"input context",
  )
  .await;
  assert_eq!(
    reused.status.code(),
    Some(0),
    "{}",
    String::from_utf8_lossy(&reused.stderr)
  );
  assert_eq!(String::from_utf8(reused.stdout).unwrap(), "done\n第二行\n");
  assert_eq!(model.requests().await.len(), 1);
  assert_eq!(node.client.list_runs(session).await.unwrap().len(), 1);
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn approval_is_not_granted_and_tool_effect_is_not_executed() {
  let model = MockModelServer::start(vec![tool_reply(
    StandardProtocol::ChatCompletions,
    "write_file",
    json!({"path":"must-not-exist.txt","content":"blocked","expected_revision":""}),
  )])
  .await
  .unwrap();
  let node = Node::start(&model).await;
  let output = invoke(&node, &["Write file", "--output-format", "jsonl"], b"").await;
  assert_eq!(
    output.status.code(),
    Some(3),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  let lines = records(&output);
  let last = lines.last().unwrap();
  assert_eq!(last["status"], "awaiting_approval");
  let id = last["run_id"].as_str().unwrap();
  assert!(!node.workspace.path().join("must-not-exist.txt").exists());
  assert_eq!(
    node.daemon.store.run(id).unwrap().unwrap().status,
    "awaiting_approval"
  );
  assert!(
    node
      .client
      .list_approvals(id)
      .await
      .unwrap()
      .iter()
      .all(|approval| approval.status == "pending")
  );
  assert_eq!(model.requests().await.len(), 1);
  node.client.cancel_run(id).await.unwrap();
  node.status(id, "cancelled").await;
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_input_has_no_daemon_or_model_side_effects() {
  let model = MockModelServer::start(Vec::new()).await.unwrap();
  let node = Node::start(&model).await;
  for input in [Vec::new(), vec![255], vec![b'x'; 1_048_577]] {
    let output = invoke(&node, &["-", "--output-format", "jsonl"], &input).await;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(records(&output).last().unwrap()["type"], "error");
  }
  assert!(node.client.list_sessions().await.unwrap().is_empty());
  assert!(model.requests().await.is_empty());
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_failure_exits_nonzero_and_is_persisted() {
  let model = MockModelServer::start(vec![ScriptedReply::status(
    400,
    json!({"error":{"message":"synthetic bad request","type":"invalid_request_error"}}),
  )])
  .await
  .unwrap();
  let node = Node::start(&model).await;
  let output = invoke(&node, &["Fail", "--output-format", "jsonl"], b"").await;
  assert_eq!(output.status.code(), Some(1));
  let lines = records(&output);
  let result = lines.last().unwrap();
  assert_eq!(result["status"], "failed");
  assert_eq!(
    node
      .daemon
      .store
      .run(result["run_id"].as_str().unwrap())
      .unwrap()
      .unwrap()
      .status,
    "failed"
  );
  assert_eq!(model.requests().await.len(), 1);
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lost_submission_ack_reports_unknown_identity_without_replay() {
  use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
  };
  let model = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let node = Node::start(&model).await;
  let session = node
    .client
    .create_session("Lost ack", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let upstream = node.endpoint().trim_start_matches("http://").to_owned();
  let store = node.daemon.store.clone();
  let session_id = session.id.clone();
  // Forward real HTTP/2 opaquely, but drop the first response after the durable
  // submission exists. Handshake and session reads pass before that boundary.
  let proxy = tokio::spawn(async move {
    let (mut client, _) = listener.accept().await.unwrap();
    let mut server = TcpStream::connect(upstream).await.unwrap();
    let (mut input, mut output) = client.split();
    let (mut incoming, mut outgoing) = server.split();
    let requests = tokio::io::copy(&mut input, &mut outgoing);
    let responses = async {
      let mut bytes = [0u8; 8192];
      loop {
        let count = incoming.read(&mut bytes).await.unwrap();
        if count == 0 || !store.runs(&session_id).unwrap().is_empty() {
          break;
        }
        output.write_all(&bytes[..count]).await.unwrap();
      }
    };
    tokio::select! { _ = requests => {}, _ = responses => {} }
  });
  let mut child = exec_support::command_at(
    &node,
    &endpoint,
    &[
      "Persist once",
      "--session",
      &session.id,
      "--idempotency-key",
      "lost-ack",
      "--output-format",
      "jsonl",
    ],
  )
  .spawn()
  .unwrap();
  drop(child.stdin.take());
  let output = exec_support::finish(child).await;
  assert_eq!(output.status.code(), Some(1));
  let lines = records(&output);
  assert!(!lines.iter().any(|line| line["type"] == "accepted"));
  let last = lines.last().unwrap();
  assert_eq!(last["type"], "error");
  assert_eq!(last["phase"], "submission");
  assert_eq!(last["submission_outcome_unknown"], true);
  assert_eq!(last["idempotency_key"], "lost-ack");
  assert_eq!(last["session_id"], session.id);
  let runs = node.run_count(&session.id, 1).await;
  node.status(&runs[0].id, "running").await;
  assert_eq!(runs[0].prompt, "Persist once");
  model.wait_for_requests(1).await;
  assert_eq!(model.requests().await.len(), 1);
  node.client.cancel_run(&runs[0].id).await.unwrap();
  node.status(&runs[0].id, "cancelled").await;
  proxy.await.unwrap();
  node.close().await;
}
