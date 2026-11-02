#[allow(dead_code)]
mod support;

use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use serde_json::json;
use std::time::Duration;
use support::{node::Node, pty::Terminal};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_terminal_activity_and_filtered_history_preserve_daemon_ownership() {
  let protocol = StandardProtocol::ChatCompletions;
  let model = MockModelServer::start_interactive().await.unwrap();
  let node = Node::start(&model).await;
  std::fs::write(
    node.workspace.path().join("fixture.txt"),
    "actual tool content 🦀",
  )
  .unwrap();
  let mut terminal = Terminal::start(&node);
  terminal.screen("Connected").await;
  terminal.send("\x0e");
  terminal.screen("New conversation").await;
  let session = node.client.list_sessions().await.unwrap().pop().unwrap();
  let prompt = "read fixture.txt and report its content";
  terminal.paste(prompt);
  terminal.send("\r");
  tokio::time::timeout(Duration::from_secs(10), model.wait_for_requests(1))
    .await
    .unwrap();
  let runs = node.run_count(&session.id, 1).await;
  assert_eq!(runs[0].prompt, prompt);
  terminal.send("\x1b[17~"); // F6 displays typed execution activity.
  terminal.screen("正在连接模型").await;
  terminal.screen("terminal-fixture").await;
  model
    .push_reply(tool_reply(
      protocol,
      "read_file",
      json!({"path":"fixture.txt"}),
    ))
    .await;
  tokio::time::timeout(Duration::from_secs(10), model.wait_for_requests(2))
    .await
    .unwrap();
  let requests = model.requests().await;
  assert!(
    requests
      .iter()
      .all(|request| request.path == protocol.path())
  );
  assert!(
    requests[1]
      .body
      .to_string()
      .contains("actual tool content 🦀")
  );
  terminal.screen("read_file").await;
  terminal.screen("actual tool").await;
  terminal.screen("🦀").await;
  terminal.screen("成功").await;
  terminal.send("\x1b[B\r"); // Select the completed tool and expand its exact arguments/result.
  terminal.screen("参数").await;
  terminal.screen("结果").await;
  terminal.snapshot("activity");
  model
    .push_reply(text_reply(
      protocol,
      "fixture content was read successfully",
    ))
    .await;
  node.status(&runs[0].id, "completed").await;
  terminal.screen("执行已完成").await;
  terminal.send("\x1b");
  terminal.screen_without("执行记录").await;
  terminal.paste("unsent next draft");
  terminal.screen("unsent next draft").await;
  assert!(!terminal.contents().contains("[200~"));
  terminal.send("\x10"); // Ctrl+P opens acknowledged input history.
  terminal.screen("输入历史").await;
  terminal.send("fixture");
  terminal.screen("搜索: fixture").await;
  terminal.snapshot("history");
  terminal.send("\r");
  terminal.screen("已回填历史输入").await;
  terminal.screen(prompt).await;
  assert_eq!(node.client.list_runs(&session.id).await.unwrap().len(), 1);
  assert_eq!(
    model.requests().await.len(),
    2,
    "history confirmation must not request model inference"
  );
  terminal.send("\x10");
  terminal.screen("输入历史").await;
  terminal.quit().await; // Global exit also works while the history picker owns input.
  assert_eq!(
    node.client.get_run(&runs[0].id).await.unwrap().status,
    "completed"
  );
  assert_eq!(model.requests().await.len(), 2);
  node.close().await;
}
