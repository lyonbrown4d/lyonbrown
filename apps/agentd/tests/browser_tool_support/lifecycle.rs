use super::{ActionMode, Runtime, approval, assert_receipts, click, open, route};
use crate::support::TestNode;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_and_closing_require_exact_approval_and_persist_mutation_receipts() {
  for operation in ["open_space", "open_tab", "close_space"] {
    for approved in [false, true] {
      let protocol = StandardProtocol::ChatCompletions;
      let call = match operation {
        "open_space" => json!({"op":operation,"profile_id":"new-fixture"}),
        "open_tab" => json!({"op":operation,"space_id":"fixture-space-1","url":"about:blank"}),
        _ => json!({"op":operation,"space_id":"fixture-space-1"}),
      };
      let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
        .await
        .unwrap();
      let main = MockModelServer::start(vec![
        tool_reply(protocol, "browser_act", json!({"call":call})),
        text_reply(protocol, "finished"),
      ])
      .await
      .unwrap();
      let node = TestNode::start(&main, &classifier, protocol).await;
      let runtime = Runtime::attach(&node.client, ActionMode::Complete).await;
      let initial = usize::from(operation != "open_space");
      if initial != 0 {
        open(&node, &runtime, &node.session_id).await;
      }
      let run = node.run("change browser fixture lifecycle").await;
      let pending = approval(&node, &run.id).await;
      assert_eq!(node.client.browser_spaces().await.unwrap().len(), initial);
      let exact: Value = serde_json::from_str(&pending.arguments_json).unwrap();
      assert_eq!(exact["call"], call);
      assert_eq!(exact["host_id"], runtime.host);
      node.client.approve(&pending.id, approved).await.unwrap();
      assert_eq!(node.terminal(&run.id).await.status, "completed");
      let expected_spaces = match operation {
        "open_space" => usize::from(approved),
        "close_space" => usize::from(!approved),
        _ => 1,
      };
      assert_eq!(
        node.client.browser_spaces().await.unwrap().len(),
        expected_spaces
      );
      if operation == "open_tab" {
        assert_eq!(
          node
            .client
            .observe_browser("fixture-tab-2", false)
            .await
            .is_ok(),
          approved
        );
      }
      assert_receipts(
        &node,
        &pending,
        if approved { "consumed" } else { "denied" },
        approved.then_some("completed"),
      );
      drop(runtime);
      node.close().await;
    }
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_pending_browser_approval_has_no_intent_or_effect() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![tool_reply(
    protocol,
    "browser_act",
    click("fixture-tab-1"),
  )])
  .await
  .unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let runtime = Runtime::attach(&node.client, ActionMode::Complete).await;
  open(&node, &runtime, &node.session_id).await;
  let run = node.run("cancel pending approval").await;
  let pending = approval(&node, &run.id).await;
  node.client.cancel_run(&run.id).await.unwrap();
  assert_eq!(node.terminal(&run.id).await.status, "cancelled");
  assert!(runtime.actions.lock().await.is_empty());
  assert_receipts(&node, &pending, "cancelled", None);
  assert_eq!(main.requests().await.len(), 1);
  drop(runtime);
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tab_discovery_lists_only_the_active_session_and_redacts_url_query() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
    .await
    .unwrap();
  let main = MockModelServer::start(vec![
    tool_reply(protocol, "browser_observe", json!({})),
    text_reply(protocol, "listed"),
  ])
  .await
  .unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let runtime = Runtime::attach(&node.client, ActionMode::Complete).await;
  let other = node
    .client
    .create_session("other", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let (_, foreign) = open(&node, &runtime, &other.id).await;
  let (_, own) = open(&node, &runtime, &node.session_id).await;
  let run = node.run("list this session's tabs").await;
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  let request = main.requests().await[1].body.to_string();
  assert!(request.contains(&own.id));
  assert!(!request.contains(&foreign.id));
  assert!(!request.contains("private-token=redacted"));
  assert!(runtime.observed.lock().await.is_empty());
  assert!(
    node
      .client
      .list_approvals(&run.id)
      .await
      .unwrap()
      .is_empty()
  );
  drop(runtime);
  node.close().await;
}
