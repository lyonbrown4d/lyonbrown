use super::{approval, command, exact, route, start};
use agent_storage::OperationStatus;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_call_identity_never_dispatches_even_with_changed_arguments() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![
    tool_reply(
      protocol,
      "execute_command",
      command(json!(["mark", "first"])),
    ),
    tool_reply(
      protocol,
      "execute_command",
      command(json!(["mark", "second"])),
    ),
  ])
  .await
  .unwrap();
  let node = start(&main, &classifier, protocol).await;
  let run = node.run("repeat fixture call identity").await;
  let pending = approval(&node, &run.id).await;
  node.client.approve(&pending.id, true).await.unwrap();
  assert_eq!(node.terminal(&run.id).await.status, "uncertain");
  assert!(node.workspace.path().join("first").exists());
  assert!(!node.workspace.path().join("second").exists());
  assert_eq!(node.client.list_approvals(&run.id).await.unwrap().len(), 1);
  assert_eq!(
    node
      .daemon
      .store
      .operation(exact(&pending)["action_id"].as_str().unwrap())
      .unwrap()
      .unwrap()
      .status,
    OperationStatus::Completed
  );
  assert_eq!(main.requests().await.len(), 2);
  node.daemon.recover().await.unwrap();
  assert!(!node.workspace.path().join("second").exists());
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ownership_change_while_approval_pending_prevents_intent_and_process() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![
    tool_reply(
      protocol,
      "execute_command",
      command(json!(["mark", "effect"])),
    ),
    text_reply(protocol, "stopped"),
  ])
  .await
  .unwrap();
  let node = start(&main, &classifier, protocol).await;
  let run = node.run("owner changes before command approval").await;
  let pending = approval(&node, &run.id).await;
  node
    .daemon
    .store
    .change_owner(&node.session_id, 1, "other-owner")
    .unwrap();
  node.client.approve(&pending.id, true).await.unwrap();
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while node.client.node().await.unwrap().active_runs != 0 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  assert!(!node.workspace.path().join("effect").exists());
  assert!(
    node
      .daemon
      .store
      .operation(exact(&pending)["action_id"].as_str().unwrap())
      .unwrap()
      .is_none()
  );
  node.close().await;
}
