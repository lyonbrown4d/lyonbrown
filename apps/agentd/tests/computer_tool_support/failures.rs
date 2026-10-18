use super::*;
use agent_test_support::{StandardProtocol, text_reply, tool_reply_with_id};
use std::sync::atomic::Ordering;

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn content_drift_before_preparation_or_execution_never_sends_input() {
  for after_approval_requested in [false, true] {
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
    let mut host = runtime.attach(&node).await;
    let run = node
      .run("Only click the unchanged observed desktop after approval")
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
    if !after_approval_requested {
      runtime.calls.content_changed.store(true, Ordering::SeqCst);
    }
    model
      .push_reply(tool_reply_with_id(
        protocol,
        "act-computer",
        "computer_act",
        args.clone(),
      ))
      .await;
    let pending = if after_approval_requested {
      let pending = approval(&node, &run.id).await;
      exact_approval(&node, &pending, &host.host_id, &args);
      assert_eq!(runtime.effects(), 0);
      runtime.calls.content_changed.store(true, Ordering::SeqCst);
      node.client.approve(&pending.id, true).await.unwrap();
      Some(pending)
    } else {
      None
    };
    wait_requests(&model, 3).await;
    // The model receives a bounded adapter-owned rejection, never a native
    // error that could contain private desktop text.
    assert!(
      model.requests().await[2]
        .body
        .to_string()
        .contains("Computer operation rejected")
    );
    model
      .push_reply(text_reply(protocol, "Stopped because the desktop changed"))
      .await;
    let result = node.terminal(&run.id).await;
    assert_eq!(result.status, "completed", "{}", result.error);
    assert!(result.result.contains("desktop changed"));
    let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
    let failed: Vec<Value> = events
      .iter()
      .filter(|event| event.kind == "tool_finished")
      .map(|event| serde_json::from_str(&event.payload_json).unwrap())
      .filter(|event: &Value| event["name"] == "computer_act")
      .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["is_error"], true);
    if let Some(pending) = pending {
      assert_receipts(&node, &pending, "consumed", Some("failed"));
    } else {
      assert!(
        node
          .client
          .list_approvals(&run.id)
          .await
          .unwrap()
          .is_empty()
      );
      let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
      assert!(!events.iter().any(|event| matches!(
        event.kind.as_str(),
        "operation_intent" | "operation_finished" | "tool_authorized"
      )));
    }
    assert_eq!(
      runtime.effects(),
      0,
      "content drift must reject before input"
    );
    assert_eq!(model.requests().await.len(), 3);
    node.daemon.recover().await.unwrap();
    assert_eq!(
      runtime.effects(),
      0,
      "recovery must not replay rejected input"
    );
    let shutdown = host.shutdown().await;
    if after_approval_requested {
      assert!(shutdown.is_err(), "native rejection closes its attachment");
    } else {
      shutdown.unwrap();
    }
    node.close().await;
  }
}
