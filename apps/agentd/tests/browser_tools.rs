mod browser_tool_support;
#[allow(dead_code)]
mod support;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use browser_tool_support::{
  ActionMode, Runtime, approval, assert_receipts, click, observation_payload, open, route, schema,
};
use serde_json::{Value, json};
use support::TestNode;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_model_protocol_observes_and_executes_only_exact_approved_browser_action() {
  for protocol in StandardProtocol::ALL {
    for approved in [false, true] {
      let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
        .await
        .unwrap();
      let main = MockModelServer::start(vec![
        tool_reply(
          protocol,
          "browser_observe",
          json!({"tab_id":"fixture-tab-1","screenshot":true}),
        ),
        tool_reply(protocol, "browser_act", click("fixture-tab-1")),
        text_reply(protocol, "fixture finished"),
      ])
      .await
      .unwrap();
      let node = TestNode::start(&main, &classifier, protocol).await;
      let runtime = Runtime::attach(&node.client, ActionMode::Complete).await;
      open(&node, &runtime, &node.session_id).await;
      let run = node.run("observe and click the local fixture").await;
      let pending = approval(&node, &run.id).await;
      assert!(
        runtime.actions.lock().await.is_empty(),
        "approval must precede browser effect"
      );
      let exact: Value = serde_json::from_str(&pending.arguments_json).unwrap();
      assert_eq!(exact["session_id"], node.session_id);
      assert_eq!(exact["host_id"], runtime.host);
      assert_eq!(exact["call"], click("fixture-tab-1")["call"]);
      let id = exact["action_id"].as_str().unwrap();
      assert_eq!(node.daemon.store.browser_action_status(id).unwrap(), None);
      node.client.approve(&pending.id, approved).await.unwrap();
      let result = node.terminal(&run.id).await;
      assert_eq!(result.status, "completed", "{}", result.error);
      let actions = runtime.actions.lock().await;
      assert_eq!(actions.len(), usize::from(approved));
      if approved {
        assert_eq!(actions[0].id, id);
        assert_eq!(actions[0].element_ref, "submit");
        assert_eq!(actions[0].tab_id, "fixture-tab-1");
      }
      drop(actions);
      assert_receipts(
        &node,
        &pending,
        if approved { "consumed" } else { "denied" },
        approved.then_some("completed"),
      );
      assert_eq!(&*runtime.observed.lock().await, &["fixture-tab-1"]);
      let requests = main.requests().await;
      assert_eq!(requests.len(), 3);
      assert!(schema(&requests[0].body, "browser_act")["properties"]["call"]["oneOf"].is_array());
      assert!(schema(&requests[0].body, "browser_observe")["properties"]["tab_id"].is_object());
      let observation = observation_payload(&requests[1].body).unwrap();
      let digest = observation["observation"]["screenshot_artifact"]
        .as_str()
        .unwrap();
      assert!(!digest.is_empty());
      let artifacts = agent_workspace::ArtifactStore::open(
        &node.daemon.config.data_path().unwrap().join("artifacts"),
        &node.daemon.node_id,
      )
      .unwrap();
      let png = artifacts.read(digest).unwrap();
      assert_eq!(image::load_from_memory(&png).unwrap().width(), 1);
      assert!(
        !requests[1]
          .body
          .to_string()
          .contains("private-token=redacted")
      );
      let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
      let plan: Value = serde_json::from_str(
        &events
          .iter()
          .find(|event| event.kind == "execution_plan")
          .unwrap()
          .payload_json,
      )
      .unwrap();
      assert_eq!(plan["task_kind"], "browser");
      assert!(
        plan["allowed_tools"]
          .as_array()
          .unwrap()
          .contains(&json!("browser_act"))
      );
      drop(runtime);
      node.close().await;
    }
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_cannot_observe_or_act_on_another_sessions_tab() {
  for tool in ["browser_observe", "browser_act"] {
    let protocol = StandardProtocol::ChatCompletions;
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let args = if tool == "browser_act" {
      click("fixture-tab-1")
    } else {
      json!({"tab_id":"fixture-tab-1"})
    };
    let main = MockModelServer::start(vec![
      tool_reply(protocol, tool, args),
      text_reply(protocol, "rejected"),
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
    open(&node, &runtime, &other.id).await;
    let run = node.run("try another session").await;
    assert_eq!(node.terminal(&run.id).await.status, "completed");
    assert!(runtime.actions.lock().await.is_empty());
    assert!(runtime.observed.lock().await.is_empty());
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    assert!(
      main.requests().await[1]
        .body
        .to_string()
        .contains("authorized browser tab unavailable")
    );
    drop(runtime);
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn takeover_or_ownership_change_while_approval_pending_prevents_browser_effect() {
  for change_owner in [false, true] {
    let protocol = StandardProtocol::ChatCompletions;
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![
      tool_reply(protocol, "browser_act", click("fixture-tab-1")),
      text_reply(protocol, "stopped"),
    ])
    .await
    .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let runtime = Runtime::attach(&node.client, ActionMode::Complete).await;
    let (space, _) = open(&node, &runtime, &node.session_id).await;
    let run = node.run("click after approval").await;
    let pending = approval(&node, &run.id).await;
    if change_owner {
      node
        .daemon
        .store
        .change_owner(&node.session_id, 1, "different-owner")
        .unwrap();
      // Human approval cannot restore authority to the adapter captured at epoch 1.
      node.client.approve(&pending.id, true).await.unwrap();
      tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while node.client.node().await.unwrap().active_runs != 0 {
          tokio::task::yield_now().await;
        }
      })
      .await
      .unwrap();
    } else {
      node.client.browser_takeover(&space.id, true).await.unwrap();
      node.client.approve(&pending.id, true).await.unwrap();
      assert_eq!(node.terminal(&run.id).await.status, "completed");
    }
    assert!(runtime.actions.lock().await.is_empty());
    assert_receipts(&node, &pending, "approved", None);
    drop(runtime);
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_disconnect_or_cancellation_after_dispatch_is_persisted_uncertain_without_replay() {
  for mode in [ActionMode::Disconnect, ActionMode::Hold] {
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
    let runtime = Runtime::attach(&node.client, mode).await;
    open(&node, &runtime, &node.session_id).await;
    let run = node.run("click with interrupted result").await;
    let pending = approval(&node, &run.id).await;
    node.client.approve(&pending.id, true).await.unwrap();
    runtime.wait_action().await;
    if matches!(mode, ActionMode::Hold) {
      node.client.cancel_run(&run.id).await.unwrap();
    }
    let result = node.terminal(&run.id).await;
    assert_eq!(result.status, "uncertain", "{}", result.error);
    assert_receipts(&node, &pending, "consumed", Some("uncertain"));
    assert_eq!(runtime.actions.lock().await.len(), 1);
    assert_eq!(
      main.requests().await.len(),
      1,
      "unknown outcomes must stop the model loop"
    );
    assert!(
      node
        .client
        .start_run(
          &node.session_id,
          "do not replay unknown browser operation",
          &agent_core::new_id()
        )
        .await
        .is_err()
    );
    node.daemon.recover().await.unwrap();
    assert_eq!(
      runtime.actions.lock().await.len(),
      1,
      "recovery must not replay browser effects"
    );
    drop(runtime);
    node.close().await;
  }
}
