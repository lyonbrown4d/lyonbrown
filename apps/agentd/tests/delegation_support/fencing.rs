use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply,
};
use serde_json::{Value, json};

use super::{Delegation, dispatch};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_parent_runs_task_cannot_be_read_reconciled_cancelled_or_accepted_by_model() {
  let protocol = StandardProtocol::ChatCompletions;
  let main = MockModelServer::start_interactive().await.unwrap();
  let worker = MockModelServer::start(vec![text_reply(protocol, "private other-parent result")])
    .await
    .unwrap();
  let node = Delegation::open(&main, &worker, protocol).await;
  let envelope = node.pair.envelope(vec!["workspace.read".into()]);
  node
    .pair
    .dispatch
    .submit(&node.pair.worker_manager.local_node(), envelope.clone())
    .await
    .unwrap();
  let original = node.pair.terminal(&envelope.task_id).await;
  node
    .pair
    .master
    .store
    .update_run_owned(
      &envelope.parent_run_id,
      node.pair.session.owner_epoch,
      agent_core::RunStatus::Completed,
      "previous parent ended",
      "",
    )
    .unwrap();
  for op in ["get", "reconcile", "cancel", "accept"] {
    let mut call = json!({"op":op,"target_id":node.pair.worker.node_id,"task_id":envelope.task_id});
    if op == "accept" {
      call["accepted"] = json!(true);
    }
    main
      .push_reply(tool_reply(protocol, "delegate_task", json!({"call":call})))
      .await;
  }
  main
    .push_reply(text_reply(protocol, "All cross-parent access was rejected"))
    .await;
  let run = node.run().await;
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  assert!(
    node
      .pair
      .master
      .store
      .approvals(&run.id)
      .unwrap()
      .is_empty()
  );
  assert_eq!(
    node
      .pair
      .master
      .store
      .task(&envelope.task_id)
      .unwrap()
      .unwrap(),
    original
  );
  assert_eq!(worker.requests().await.len(), 1);
  let requests = main.requests().await;
  assert_eq!(requests.len(), 5);
  assert!(
    !requests
      .last()
      .unwrap()
      .body
      .to_string()
      .contains("private other-parent result")
  );
  assert!(
    requests
      .last()
      .unwrap()
      .body
      .to_string()
      .contains("outside this parent run")
  );
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_cancel_response_is_durable_uncertain_and_never_replays_worker_execution() {
  let protocol = StandardProtocol::Responses;
  let main = MockModelServer::start_interactive().await.unwrap();
  let worker = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let mut node = Delegation::open(&main, &worker, protocol).await;
  main
    .push_reply(tool_reply(
      protocol,
      "delegate_task",
      dispatch(&node.pair.worker.node_id),
    ))
    .await;
  let run = node.run().await;
  let first = node.approval(&run.id, 1).await;
  let exact: Value = serde_json::from_str(&first.arguments_json).unwrap();
  let id = exact["envelope"]["task_id"].as_str().unwrap();
  node.client.approve(&first.id, true).await.unwrap();
  tokio::time::timeout(
    std::time::Duration::from_secs(5),
    worker.wait_for_requests(1),
  )
  .await
  .unwrap();
  main
    .push_reply(tool_reply(
      protocol,
      "delegate_task",
      json!({"call":{
        "op":"cancel","target_id":node.pair.worker.node_id,"task_id":id
      }}),
    ))
    .await;
  let second = node.approval(&run.id, 2).await;
  assert_eq!(
    node.pair.master.store.task(id).unwrap().unwrap().status,
    "running"
  );
  node.pair.abort_worker_listener().await;
  node.client.approve(&second.id, true).await.unwrap();
  let run = node.terminal(&run.id).await;
  assert_eq!(run.status, "uncertain", "{}", run.error);
  assert_eq!(
    node.pair.master.store.task(id).unwrap().unwrap().status,
    "uncertain"
  );
  assert_eq!(
    main.requests().await.len(),
    2,
    "uncertainty must stop the Harness model loop"
  );
  assert_eq!(worker.requests().await.len(), 1);
  node.pair.master.recover().await.unwrap();
  assert_eq!(
    worker.requests().await.len(),
    1,
    "recovery must never replay an attempt"
  );
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_cannot_supply_endpoint_or_expand_configured_capabilities() {
  let protocol = StandardProtocol::Anthropic;
  let main = MockModelServer::start_interactive().await.unwrap();
  let worker = MockModelServer::start(Vec::new()).await.unwrap();
  let node = Delegation::open(&main, &worker, protocol).await;
  let mut injected = dispatch(&node.pair.worker.node_id);
  injected["call"]["endpoint"] = json!("https://127.0.0.1:1");
  let mut expanded = dispatch(&node.pair.worker.node_id);
  expanded["call"]["capabilities"] = json!(["workspace.write.approval"]);
  for args in [injected, expanded, dispatch("unconfigured-worker")] {
    main
      .push_reply(tool_reply(protocol, "delegate_task", args))
      .await;
  }
  main
    .push_reply(text_reply(protocol, "No unauthorized delegation"))
    .await;
  let run = node.run().await;
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  assert!(
    node
      .pair
      .master
      .store
      .approvals(&run.id)
      .unwrap()
      .is_empty()
  );
  assert!(
    node
      .pair
      .master
      .store
      .tasks_for_session(&node.pair.session.id, "", 100)
      .unwrap()
      .is_empty()
  );
  assert!(worker.requests().await.is_empty());
  node.close().await;
}
