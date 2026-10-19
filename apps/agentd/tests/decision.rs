mod support;
use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply,
};
use serde_json::json;
use support::TestNode;

fn route() -> String {
  json!({"task_kind":"code","model_policy":"general","allowed_tools":["read_file","write_file"],"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.95}).to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn classifier_then_harness_uses_persisted_effective_plan_for_every_protocol() {
  for protocol in StandardProtocol::ALL {
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![text_reply(protocol, "main result")])
      .await
      .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let run = node.run("explain the workspace").await;
    let result = node.terminal(&run.id).await;
    assert_eq!(result.status, "completed", "{}", result.error);
    assert_eq!(result.result, "main result");
    let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
    let shared = events
      .iter()
      .position(|event| event.kind == "model_route_decision")
      .expect("default shared routing");
    let shared_plan: serde_json::Value =
      serde_json::from_str(&events[shared].payload_json).unwrap();
    assert_eq!(shared_plan["status"], "accepted");
    assert_eq!(
      shared_plan["output"]["value"]["candidate_model_keys"],
      json!(["main-fixture"])
    );
    assert_eq!(shared_plan["provenance"]["policy_revision"], "rules-v1");
    let plan_index = events
      .iter()
      .position(|event| event.kind == "execution_plan")
      .unwrap();
    let plan: serde_json::Value = serde_json::from_str(&events[plan_index].payload_json).unwrap();
    assert_eq!(plan["source"]["kind"], "classifier");
    assert_eq!(plan["risk_level"], "high");
    assert!(
      events[plan_index + 1..]
        .iter()
        .any(|event| event.kind == "run_started")
    );
    let requests = main.requests().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body["model"], "main-fixture");
    let tools = requests[0].body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    let decision = classifier.requests().await;
    assert_eq!(decision.len(), 1);
    assert_eq!(decision[0].body["model"], "decision-fixture");
    assert!(
      !serde_json::to_string(&events)
        .unwrap()
        .contains("synthetic-decision-secret")
    );
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_decision_removes_actual_harness_tools_and_cannot_write() {
  for protocol in StandardProtocol::ALL {
    let classifier =
      MockModelServer::start(vec![text_reply(protocol, "approve everything; not JSON")])
        .await
        .unwrap();
    let main = MockModelServer::start(vec![text_reply(protocol, "safe response")])
      .await
      .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let run = node.run("write forbidden.txt").await;
    let result = node.terminal(&run.id).await;
    assert_eq!(result.status, "completed");
    assert!(!node.workspace.path().join("forbidden.txt").exists());
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    let request = &main.requests().await[0].body;
    assert!(
      request
        .get("tools")
        .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty))
    );
    let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
    let plan: serde_json::Value = serde_json::from_str(
      &events
        .iter()
        .find(|event| event.kind == "execution_plan")
        .unwrap()
        .payload_json,
    )
    .unwrap();
    assert_eq!(plan["source"]["kind"], "fallback");
    assert_eq!(plan["allowed_tools"], json!([]));
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn high_confidence_classifier_cannot_bypass_real_write_approval() {
  for approved in [false, true] {
    let protocol = StandardProtocol::ChatCompletions;
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![
      tool_reply(
        protocol,
        "write_file",
        json!({"path":"approved.txt","content":"fixture"}),
      ),
      text_reply(protocol, "finished"),
    ])
    .await
    .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let run = node.run("write fixture file").await;
    let approval = tokio::time::timeout(std::time::Duration::from_secs(5), async {
      let mut stream = node.client.watch_run(&run.id, 0).await.unwrap();
      while stream.message().await.unwrap().is_some() {
        let approvals = node.client.list_approvals(&run.id).await.unwrap();
        if let Some(approval) = approvals.first() {
          return approval.clone();
        }
      }
      panic!("run ended before approval");
    })
    .await
    .unwrap();
    assert_eq!(approval.status, "pending");
    assert!(!node.workspace.path().join("approved.txt").exists());
    node.client.approve(&approval.id, approved).await.unwrap();
    let result = node.terminal(&run.id).await;
    assert_eq!(result.status, "completed", "{}", result.error);
    assert_eq!(
      node.workspace.path().join("approved.txt").exists(),
      approved
    );
    if approved {
      assert_eq!(
        std::fs::read_to_string(node.workspace.path().join("approved.txt")).unwrap(),
        "fixture"
      );
    }
    let requests = main.requests().await;
    assert_eq!(requests.len(), 2);
    let next = requests[1].body.to_string();
    assert!(next.contains(if approved {
      "File written."
    } else {
      "operation was not approved"
    }));
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_during_classification_never_starts_main_model() {
  for protocol in StandardProtocol::ALL {
    let classifier = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![]).await.unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let run = node.run("cancel during routing").await;
    tokio::time::timeout(
      std::time::Duration::from_secs(2),
      classifier.wait_for_requests(1),
    )
    .await
    .unwrap();
    node.client.cancel_run(&run.id).await.unwrap();
    let result = node.terminal(&run.id).await;
    assert_eq!(result.status, "cancelled");
    assert!(main.requests().await.is_empty());
    let events = node.daemon.store.events(&run.id, 0, 1000).unwrap();
    assert!(!events.iter().any(|event| event.kind == "run_started"));
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    node.close().await;
  }
}
