#[allow(dead_code)]
mod support;
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use serde_json::json;
use support::{node::Node, pty::Terminal};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_dialog_probes_saves_and_explicitly_selects_the_real_daemon_binding() {
  let protocol = StandardProtocol::ChatCompletions;
  let model = MockModelServer::start(vec![
    ScriptedReply::status(200, json!({"data":[{"id":"terminal-custom"}]})),
    ScriptedReply::stall(),
    text_reply(protocol, "custom provider completed"),
  ])
  .await
  .unwrap();
  let node = Node::start(&model).await;
  let mut terminal = Terminal::start(&node);
  terminal.screen("Connected").await;
  terminal.paste("/providers");
  terminal.send("\r");
  terminal.screen("新增 Provider").await;
  terminal.send("\r");
  terminal.screen("自定义 Provider").await;
  terminal.paste("PTY Provider");
  terminal.send("\t\x1b[C\t"); // Protocol: Responses -> Chat Completions; focus URL.
  terminal.paste(&format!("{}/v1", model.base_url()));
  terminal.send("\t");
  terminal.paste("terminal-custom");
  terminal.send("\t");
  let key = "synthetic-dialog-key-only";
  terminal.paste(key);
  terminal.send("\t\r"); // Probe, never save implicitly.
  terminal.screen("连接成功").await;
  assert!(!terminal.contents().contains(key));
  terminal.snapshot("provider-profile");
  assert!(
    node
      .client
      .list_provider_profiles()
      .await
      .unwrap()
      .is_empty()
  );
  assert!(node.client.list_sessions().await.unwrap().is_empty());
  let requests = model.requests().await;
  assert_eq!(requests.len(), 1);
  assert_eq!(requests[0].method, "GET");
  assert_eq!(requests[0].path, "/v1/models");
  terminal.send("\t\r");
  terminal.screen("已保存").await;
  let profiles = node.client.list_provider_profiles().await.unwrap();
  assert_eq!(profiles.len(), 1);
  assert_eq!(profiles[0].name, "PTY Provider");
  assert_eq!(profiles[0].revision, 1);
  assert!(profiles[0].credential_configured);
  let events = node
    .daemon
    .store
    .events(&format!("provider:{}", profiles[0].id), 0, 100)
    .unwrap();
  assert_eq!(events.len(), 1);
  assert!(!serde_json::to_string(&events).unwrap().contains(key));
  terminal.send("\x1b[Z\r"); // Save -> Probe; use the stored secret for a stalled second probe.
  tokio::time::timeout(
    std::time::Duration::from_secs(5),
    model.wait_for_requests(2),
  )
  .await
  .unwrap();
  let cancelled_at = std::time::Instant::now();
  terminal.send("\x1b");
  terminal.screen_without("自定义 Provider").await;
  terminal.send("\x0e");
  terminal.screen("New conversation").await;
  assert!(
    cancelled_at.elapsed() < std::time::Duration::from_secs(4),
    "probe must not block other commands until its five-second timeout"
  );
  let session = node.client.list_sessions().await.unwrap().pop().unwrap();
  terminal.paste("/model");
  terminal.send("\r");
  terminal.screen("选择模型").await;
  terminal.send("PTY Provider");
  terminal.screen("PTY Provider").await;
  terminal.send("\r");
  terminal.screen("Session model selected").await;
  terminal.paste("hello from dialog");
  terminal.send("\r");
  let runs = node.run_count(&session.id, 1).await;
  node.status(&runs[0].id, "completed").await;
  let requests = model.requests().await;
  assert_eq!(requests.len(), 3);
  assert_eq!(requests[2].path, protocol.path());
  assert_eq!(requests[2].body["model"], "terminal-custom");
  assert_eq!(
    requests[2].headers.get("authorization").map(String::as_str),
    Some("Bearer synthetic-dialog-key-only")
  );
  terminal.quit().await;
  node.close().await;
}
