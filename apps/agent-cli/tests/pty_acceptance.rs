mod support;

use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply,
};
use serde_json::json;
use std::time::Duration;
use support::{node::Node, pty::Terminal};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_terminal_auth_history_multiline_resize_reconnect_and_cleanup() {
  let protocol = StandardProtocol::ChatCompletions;
  let model = MockModelServer::start(vec![text_reply(protocol, "终端回复 🦀\nsecond line")])
    .await
    .unwrap();
  let mut node = Node::start(&model).await;
  let mut terminal = Terminal::start(&node);
  terminal.screen("Connected").await;
  terminal.send("\x0e");
  terminal.screen("New conversation").await;
  let session = node.client.list_sessions().await.unwrap().pop().unwrap();
  let pasted = "你好 🦀\nsecond input line";
  let prompt = format!("{pasted}\nkeyboard newline");
  terminal.paste("你好错 🦀");
  // Real terminal Home/Right/Delete events edit within Unicode text.
  terminal.send("\x1b[H\x1b[C\x1b[C\x1b[3~");
  terminal.screen("你好 🦀").await;
  terminal.send("\x1b[F");
  terminal.paste("\nsecond input line");
  terminal.screen("second input line").await;
  terminal.send("\x1b\r"); // Alt+Enter inserts a newline instead of submitting.
  terminal.paste("keyboard newline");
  terminal.send("\r");
  let runs = node.run_count(&session.id, 1).await;
  let run = node.status(&runs[0].id, "completed").await;
  assert_eq!(run.prompt, prompt);
  assert_eq!(run.result, "终端回复 🦀\nsecond line");
  terminal.screen("终端回复 🦀").await;
  terminal.snapshot("conversation");
  let requests = model.requests().await;
  assert_eq!(requests.len(), 1);
  assert_eq!(requests[0].path, protocol.path());
  assert!(requests[0].body.to_string().contains("second input line"));

  terminal.send("\x0e");
  tokio::time::timeout(Duration::from_secs(5), async {
    while node.client.list_sessions().await.unwrap().len() != 2 {
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  })
  .await
  .unwrap();
  terminal.screen("Tasks keep running").await;
  // Tab switches back to the previous durable conversation.
  terminal.send("\t");
  terminal.screen("终端回复 🦀").await;
  terminal.resize(8, 35);
  terminal.screen("Lyonbrown").await;
  terminal.screen_without("Sessions").await;
  terminal.snapshot("compact");
  terminal.resize(36, 140);
  terminal.screen("Conversation").await;
  terminal.screen("终端回复 🦀").await;

  node.disconnect().await;
  terminal.screen("Disconnected").await;
  terminal.paste("draft survives reconnect");
  terminal.send("\r");
  terminal.screen("draft is preserved").await;
  node.reconnect().await;
  terminal.send("\x12");
  terminal.screen("Connected").await;
  terminal.screen("draft survives reconnect").await;
  assert_eq!(node.client.list_runs(&session.id).await.unwrap().len(), 1);
  assert_eq!(
    model.requests().await.len(),
    1,
    "reconnect must never replay submission"
  );
  terminal.quit().await;
  assert_eq!(
    node.client.get_run(&run.id).await.unwrap().status,
    "completed"
  );
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_terminal_approval_denial_cancel_and_quit_preserve_daemon_jobs() {
  let protocol = StandardProtocol::ChatCompletions;
  let model = MockModelServer::start(vec![
    tool_reply(
      protocol,
      "write_file",
      json!({"path":"approved.txt","content":"批准 🦀"}),
    ),
    text_reply(protocol, "approved operation finished"),
    tool_reply(
      protocol,
      "write_file",
      json!({"path":"denied.txt","content":"must not exist"}),
    ),
    text_reply(protocol, "denied operation finished"),
    // Cross the real protocol adapter's delta batching threshold before holding the stream.
    ScriptedReply::sse(vec![json!({"choices":[{"delta":{"content":format!("streamed before cancellation {}", "x".repeat(80))},"finish_reason":null}]})]).stall_after(),
    ScriptedReply::stall(),
  ])
  .await
  .unwrap();
  let mut node = Node::start(&model).await;
  let mut terminal = Terminal::start(&node);
  terminal.screen("Connected").await;
  terminal.send("\x0e");
  terminal.screen("New conversation").await;
  let session = node.client.list_sessions().await.unwrap().pop().unwrap();

  for (index, approved, file, key) in [
    (1, true, "approved.txt", "\x19"),
    (2, false, "denied.txt", "\x05"),
  ] {
    terminal.paste(&format!("write fixture {file}"));
    terminal.send("\r");
    let runs = node.run_count(&session.id, index).await;
    let run = runs.iter().find(|run| run.prompt.ends_with(file)).unwrap();
    node.status(&run.id, "awaiting_approval").await;
    terminal.screen("write_file").await;
    let approval = node
      .client
      .list_approvals(&run.id)
      .await
      .unwrap()
      .pop()
      .unwrap();
    assert_eq!(approval.status, "pending");
    assert!(!node.workspace.path().join(file).exists());
    terminal.snapshot(if approved { "approve" } else { "deny" });
    terminal.send(key);
    let result = node.status(&run.id, "completed").await;
    assert!(result.error.is_empty(), "{}", result.error);
    assert_eq!(node.workspace.path().join(file).exists(), approved);
    if approved {
      assert_eq!(
        std::fs::read_to_string(node.workspace.path().join(file)).unwrap(),
        "批准 🦀"
      );
    }
    let recorded = node.client.list_approvals(&run.id).await.unwrap();
    assert_eq!(
      recorded[0].status,
      if approved { "consumed" } else { "denied" }
    );
    terminal
      .screen(if approved {
        "approved operation finished"
      } else {
        "denied operation finished"
      })
      .await;
  }

  terminal.paste("wait until cancelled");
  terminal.send("\r");
  let runs = node.run_count(&session.id, 3).await;
  let cancelled = runs
    .iter()
    .find(|run| run.prompt == "wait until cancelled")
    .unwrap();
  tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(5))
    .await
    .unwrap();
  terminal.screen("streamed before cancellation").await;
  assert!(
    node
      .client
      .get_run(&cancelled.id)
      .await
      .unwrap()
      .result
      .is_empty(),
    "live text must appear before a completed durable result"
  );
  terminal.snapshot("streaming");
  terminal.resize(8, 35);
  terminal.screen_without("Sessions").await;
  terminal.screen("streamed before cancellation").await;
  terminal.snapshot("compact-streaming");
  terminal.resize(36, 140);
  terminal.screen("Conversation").await;
  node.disconnect().await;
  terminal.screen("Disconnected").await;
  node.reconnect().await;
  terminal.send("\x12");
  terminal.screen("Connected").await;
  terminal.screen("streamed before cancellation").await;
  assert_eq!(
    terminal
      .contents()
      .matches("streamed before cancellation")
      .count(),
    1
  );
  assert_eq!(
    model.requests().await.len(),
    5,
    "resubscribing to persisted events must not repeat inference"
  );
  terminal.send("\x18");
  node.status(&cancelled.id, "cancelled").await;
  terminal.screen("cancelled").await;

  terminal.paste("keep daemon job alive on quit");
  terminal.send("\r");
  let runs = node.run_count(&session.id, 4).await;
  let background = runs
    .iter()
    .find(|run| run.prompt == "keep daemon job alive on quit")
    .unwrap();
  tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(6))
    .await
    .unwrap();
  terminal.quit().await;
  assert_eq!(
    node.client.get_run(&background.id).await.unwrap().status,
    "running"
  );
  node.client.cancel_run(&background.id).await.unwrap();
  node.status(&background.id, "cancelled").await;
  assert_eq!(model.requests().await.len(), 6);
  node.close().await;
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
