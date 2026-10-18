//! Local model protocols through the real Harness and synthetic desktop runtime.
//! These tests perform no OS input and do not constitute native acceptance.
mod computer_tool_support;
#[allow(dead_code)]
mod support;

use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply_with_id};
use computer_tool_support::*;
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};
use support::TestNode;

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_protocol_observes_and_requires_exact_approval_before_computer_effect() {
  for protocol in StandardProtocol::ALL {
    for approved in [false, true] {
      let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
        .await
        .unwrap();
      let model = MockModelServer::start_interactive().await.unwrap();
      let node = TestNode::start_configured(&model, &classifier, protocol, |config| {
        config.computer_tools = true;
        config.run_timeout_secs = 45;
        config.computer_lease_secs = 10;
      })
      .await;
      let runtime = Runtime::new(&node);
      let mut host = runtime.attach(&node).await;
      let run = node
        .run("Observe the synthetic desktop and click its button after approval")
        .await;
      model
        .push_reply(tool_reply_with_id(
          protocol,
          "observe-computer",
          "computer_observe",
          json!({"screenshot":false}),
        ))
        .await;
      let observed = observation(&model).await;
      assert_eq!(observed["untrusted_desktop_content"], true);
      let snapshot = &observed["observation"];
      assert!(uuid::Uuid::parse_str(snapshot["snapshot_id"].as_str().unwrap()).is_ok());
      assert!(snapshot["revision"].as_u64().unwrap() > 0);
      assert_eq!(
        snapshot["bounds"],
        json!({"x":0,"y":0,"width":80,"height":80})
      );
      assert_eq!(snapshot["windows"][0]["id"], 7);
      assert_eq!(snapshot["accessibility"]["nodes"][0]["role"], "button");
      assert_eq!(snapshot["screenshot_artifact"], "");
      assert_eq!(snapshot["scope"], json!({"Window": 7}));
      assert!(!observed.to_string().contains("rgba"));
      if approved && protocol == StandardProtocol::ChatCompletions {
        // Deliberately exceed the runtime snapshot TTL: model latency must refresh
        // the original unchanged content before immutable preparation.
        tokio::time::sleep(Duration::from_secs(12)).await;
      }
      let args = click(&observed);
      model
        .push_reply(tool_reply_with_id(
          protocol,
          "act-computer",
          "computer_act",
          args.clone(),
        ))
        .await;
      let pending = approval(&node, &run.id).await;
      let exact = exact_approval(&node, &pending, &host.host_id, &args);
      assert_eq!(runtime.effects(), 0, "no effect before human approval");
      if approved && protocol == StandardProtocol::ChatCompletions {
        // Approval latency must also refresh unchanged content before execution.
        tokio::time::sleep(Duration::from_secs(12)).await;
      }
      node.client.approve(&pending.id, approved).await.unwrap();
      wait_requests(&model, 3).await;
      model
        .push_reply(text_reply(protocol, "Synthetic desktop finished"))
        .await;
      let result = node.terminal(&run.id).await;
      assert_eq!(result.status, "completed", "{}", result.error);
      assert!(result.result.contains("Synthetic desktop finished"));
      assert_eq!(runtime.effects(), usize::from(approved));
      if approved {
        assert!(matches!(
          runtime.calls.effects.lock().unwrap()[0],
          agent_computer::DesktopAction::Click {
            x: 10,
            y: 10,
            button: agent_computer::MouseButton::Left
          }
        ));
        assert!(
          runtime.calls.captures.load(Ordering::SeqCst) >= 3,
          "fresh captures at observe, preparation and execution"
        );
      }
      assert_receipts(
        &node,
        &pending,
        if approved { "consumed" } else { "denied" },
        approved.then_some("completed"),
      );
      assert!(uuid::Uuid::parse_str(exact["action_id"].as_str().unwrap()).is_ok());
      let requests = model.requests().await;
      assert_eq!(requests.len(), 3);
      assert!(
        requests
          .iter()
          .all(|request| request.path == protocol.path())
      );
      assert!(
        !requests[0]
          .body
          .to_string()
          .contains("synthetic-main-secret")
      );
      host.shutdown().await.unwrap();
      node.close().await;
    }
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn lost_or_corrupted_execute_receipt_fences_run_and_persists_uncertain_without_replay() {
  for corrupt in [false, true] {
    let protocol = StandardProtocol::ChatCompletions;
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let model = MockModelServer::start_interactive().await.unwrap();
    let node = TestNode::start_configured(&model, &classifier, protocol, |config| {
      config.computer_tools = true
    })
    .await;
    let runtime = Runtime::new(&node);
    let host = if corrupt {
      LostReceiptHost::attach_corrupted(&node, &runtime).await
    } else {
      LostReceiptHost::attach(&node, &runtime).await
    };
    let run = node
      .run("Click synthetic desktop with a lost execution receipt")
      .await;
    model
      .push_reply(tool_reply_with_id(
        protocol,
        "observe-computer",
        "computer_observe",
        json!({"screenshot":false}),
      ))
      .await;
    let args = click(&observation(&model).await);
    model
      .push_reply(tool_reply_with_id(
        protocol,
        "act-computer",
        "computer_act",
        args.clone(),
      ))
      .await;
    let pending = approval(&node, &run.id).await;
    exact_approval(&node, &pending, &host.host_id, &args);
    assert_eq!(runtime.effects(), 0);
    node.client.approve(&pending.id, true).await.unwrap();
    host.wait_lost().await;
    runtime.wait_effect().await;
    if !corrupt {
      node.client.cancel_run(&run.id).await.unwrap();
    }
    let result = node.terminal(&run.id).await;
    assert_eq!(result.status, "uncertain", "{}", result.error);
    assert_receipts(&node, &pending, "consumed", Some("uncertain"));
    assert_eq!(runtime.effects(), 1);
    assert_eq!(
      model.requests().await.len(),
      2,
      "unknown execution must stop model continuation"
    );
    assert!(
      node
        .client
        .start_run(&node.session_id, "never replay", &agent_core::new_id())
        .await
        .is_err()
    );
    node.daemon.recover().await.unwrap();
    assert_eq!(runtime.effects(), 1, "recovery must preserve effect count");
    assert_eq!(
      node.daemon.store.run(&run.id).unwrap().unwrap().status,
      "uncertain"
    );
    assert_receipts(&node, &pending, "consumed", Some("uncertain"));
    drop(host);
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn classifier_cannot_grant_computer_tools_without_config_host_and_input_permission() {
  for (enabled, attached, input_denied) in [
    (false, true, false),
    (true, false, false),
    (true, true, true),
  ] {
    let protocol = StandardProtocol::ChatCompletions;
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let model = MockModelServer::start(vec![text_reply(protocol, "No authorized computer tool")])
      .await
      .unwrap();
    let node = TestNode::start_configured(&model, &classifier, protocol, |config| {
      config.computer_tools = enabled
    })
    .await;
    let runtime = Runtime::new(&node);
    runtime
      .calls
      .input_denied
      .store(input_denied, Ordering::SeqCst);
    let host = if attached {
      Some(runtime.attach(&node).await)
    } else {
      None
    };
    let run = node.run("Observe the computer").await;
    assert_eq!(node.terminal(&run.id).await.status, "completed");
    let body: Value = model.requests().await[0].body.clone();
    assert!(
      !body
        .get("tools")
        .unwrap_or(&Value::Null)
        .to_string()
        .contains("computer_")
    );
    assert_eq!(runtime.effects(), 0);
    assert_eq!(runtime.calls.captures.load(Ordering::SeqCst), 0);
    let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
    let plan: Value = serde_json::from_str(
      &events
        .iter()
        .find(|event| event.kind == "execution_plan")
        .unwrap()
        .payload_json,
    )
    .unwrap();
    assert!(!plan["allowed_tools"].to_string().contains("computer_"));
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    drop(host);
    node.close().await;
  }
}
