mod support;

use agent_core::RunStatus;
use agent_protocol::v1::{self, client_service_server::ClientService};
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use serde_json::json;
use support::TestNode;
use tonic::Request;

fn route(protocol: StandardProtocol) -> ScriptedReply {
  text_reply(protocol, &json!({"task_kind":"code","model_policy":"general","allowed_tools":[],"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.99}).to_string())
}

fn interrupted(node: &TestNode) -> agent_core::Run {
  let (run, _) = node
    .daemon
    .store
    .accept_run(
      &node.session_id,
      1,
      "Never replay interrupted side effects",
      "original",
    )
    .unwrap();
  node
    .daemon
    .store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .unwrap();
  node
    .daemon
    .store
    .commit_snapshot(
      &node.session_id,
      1,
      b"unreadable interrupted history",
      "snapshot",
      json!({}),
    )
    .unwrap();
  run
}

fn recovery(id: &str) -> v1::RecoverRunRequest {
  v1::RecoverRunRequest {
    run_id: id.into(),
    owner_epoch: 1,
    action: v1::RunRecoveryAction::ContinueNewSession.into(),
    idempotency_key: "explicit-recovery".into(),
    note: "Manually verified prior effects; summarize only".into(),
    prompt: "Summarize the manually verified result".into(),
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_recovery_runs_fresh_prompt_through_all_real_adapters_without_replay() {
  for protocol in StandardProtocol::ALL {
    let classifier = MockModelServer::start(vec![route(protocol), route(protocol)])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![text_reply(protocol, "verified summary")])
      .await
      .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    // A prior committed session becomes unreadable. A real StartRun must mark
    // the failed Harness restoration Uncertain, making explicit recovery usable.
    let (seed, _) = node
      .daemon
      .store
      .accept_run(&node.session_id, 1, "prior history", "seed")
      .unwrap();
    node
      .daemon
      .store
      .update_run_owned(&seed.id, 1, RunStatus::Completed, "prior result", "")
      .unwrap();
    node
      .daemon
      .store
      .commit_snapshot(
        &node.session_id,
        1,
        b"unreadable interrupted history",
        "snapshot",
        json!({}),
      )
      .unwrap();
    let run = node.run("Never replay interrupted side effects").await;
    assert_eq!(node.terminal(&run.id).await.status, "uncertain");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
      while node.client.node().await.unwrap().active_runs != 0 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
    let original = node.daemon.store.run(&run.id).unwrap().unwrap();
    assert_eq!(original.status, "uncertain");
    let report = node.client.run_recovery(&run.id).await.unwrap();
    assert!(report.fresh_session_required);
    assert!(report.reason.contains("snapshot inspection failed"));
    assert_eq!(report.allowed_actions.len(), 2);
    assert!(main.requests().await.is_empty());
    let request = recovery(&run.id);
    let result = node.client.recover_run(request.clone()).await.unwrap();
    let fresh = result.session.unwrap();
    assert_eq!(fresh.workspace, node.workspace.path().to_str().unwrap());
    let replacement = result.run.unwrap();
    assert_ne!(fresh.id, node.session_id);
    assert_eq!(
      node.terminal(&replacement.id).await.result,
      "verified summary"
    );
    let repeat = node.client.recover_run(request).await.unwrap();
    assert_eq!(repeat.session.unwrap().id, fresh.id);
    assert_eq!(repeat.run.unwrap().id, replacement.id);
    assert_eq!(node.daemon.store.run(&run.id).unwrap(), Some(original));
    assert_eq!(
      node.daemon.store.load_snapshot(&node.session_id).unwrap(),
      Some(b"unreadable interrupted history".to_vec())
    );
    assert!(
      node
        .daemon
        .store
        .load_snapshot(&fresh.id)
        .unwrap()
        .is_some()
    );
    assert!(
      node
        .daemon
        .store
        .approvals(&replacement.id)
        .unwrap()
        .is_empty()
    );
    let report = node.client.run_recovery(&run.id).await.unwrap();
    assert!(report.allowed_actions.is_empty());
    assert_eq!(report.resolution.unwrap().run.unwrap().status, "completed");
    let requests = main.requests().await;
    assert_eq!(requests.len(), 1);
    let body = requests[0].body.to_string();
    assert!(body.contains("Summarize the manually verified result"));
    assert!(!body.contains("Never replay interrupted side effects"));
    assert!(!body.contains("unreadable interrupted history"));
    let mut changed = recovery(&run.id);
    changed.idempotency_key = "different-recovery".into();
    assert!(node.client.recover_run(changed).await.is_err());
    assert_eq!(node.daemon.store.sessions().unwrap().len(), 2);
    node.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_fresh_intent_recovers_after_restart_and_can_be_cancelled() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![route(protocol)]).await.unwrap();
  let main = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let run = interrupted(&node);
  node.daemon.recover().await.unwrap();
  let original = node.daemon.store.run(&run.id).unwrap().unwrap();
  // Simulate a crash after durable recovery admission and before launch.
  let (resolution, _) = node
    .daemon
    .store
    .recover_run(agent_storage::RecoverRun {
      run_id: &run.id,
      owner_epoch: 1,
      node_id: &node.daemon.node_id,
      action: agent_storage::RecoveryAction::ContinueNewSession,
      idempotency_key: "durable-recovery",
      note: "Human verified; no replay",
      prompt: "Wait for further instructions",
    })
    .unwrap();
  let id = resolution.replacement_run_id.unwrap();
  node.daemon.recover().await.unwrap();
  main.wait_for_requests(1).await;
  node
    .daemon
    .cancel_run(Request::new(v1::RunRequest { run_id: id.clone() }))
    .await
    .unwrap();
  assert_eq!(node.terminal(&id).await.status, "cancelled");
  assert_eq!(node.daemon.store.run(&run.id).unwrap(), Some(original));
  assert_eq!(main.requests().await.len(), 1);
  assert!(node.daemon.store.pending_runs().unwrap().is_empty());
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_and_stale_recovery_requests_cannot_create_execution() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![route(protocol)]).await.unwrap();
  let main = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let run = node.run("active request").await;
  main.wait_for_requests(1).await;
  assert!(node.client.recover_run(recovery(&run.id)).await.is_err());
  node.client.cancel_run(&run.id).await.unwrap();
  assert_eq!(node.terminal(&run.id).await.status, "cancelled");
  // A separate uncertain run with a changed epoch also rejects recovery.
  let (unknown, _) = node
    .daemon
    .store
    .accept_run(&node.session_id, 1, "unknown", "unknown")
    .unwrap();
  node
    .daemon
    .store
    .update_run_owned(&unknown.id, 1, RunStatus::Uncertain, "", "unknown")
    .unwrap();
  node
    .daemon
    .store
    .change_owner(&node.session_id, 1, &node.daemon.node_id)
    .unwrap();
  assert!(
    node
      .client
      .recover_run(recovery(&unknown.id))
      .await
      .is_err()
  );
  assert!(
    node
      .daemon
      .store
      .run_recovery(&unknown.id)
      .unwrap()
      .is_none()
  );
  assert_eq!(node.daemon.store.sessions().unwrap().len(), 1);
  assert_eq!(main.requests().await.len(), 1);
  node.close().await;
}
