mod support;
use agent_protocol::v1;
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use std::time::Duration;
use support::TestNode;
async fn requested(mock: &MockModelServer, count: usize) {
  tokio::time::timeout(Duration::from_secs(5), mock.wait_for_requests(count))
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uncertain_model_cancellation_fences_inherited_queue_and_new_run_admission() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start_interactive().await.unwrap();
  let main = MockModelServer::start_interactive().await.unwrap();
  main.push_reply(ScriptedReply::stall()).await;
  classifier
    .push_reply(text_reply(protocol, "invalid decision"))
    .await;
  let node = TestNode::start(&main, &classifier, protocol).await;
  let first = node.run("first run").await;
  requested(&main, 1).await;
  let receipt = node
    .client
    .submit_run_input(v1::RunInputRequest {
      run_id: first.id.clone(),
      owner_epoch: 1,
      request_id: agent_core::new_id(),
      kind: v1::RunInputKind::FollowUp as i32,
      prompt: "retained input must not be consumed before reconciliation".into(),
    })
    .await
    .unwrap();
  node.client.cancel_run(&first.id).await.unwrap();
  assert_eq!(node.terminal(&first.id).await.status, "uncertain");
  let before = node
    .daemon
    .store
    .load_snapshot(&node.session_id)
    .await
    .unwrap()
    .unwrap();
  let snapshot: serde_json::Value = serde_json::from_slice(&before).unwrap();
  assert_eq!(snapshot["active"]["in_flight"], "model");
  let queued = node.client.list_run_inputs(&first.id).await.unwrap();
  assert_eq!(queued.len(), 1);
  assert_eq!(queued[0].entry_id, receipt.entry_id);
  assert_eq!(queued[0].run_id, first.id);
  assert_eq!(queued[0].state, v1::RunInputState::Pending as i32);
  // Unknown main-model IO is not a known cancellation boundary. Starting a
  // successor would hand the old queue to a new Runtime before reconciliation.
  assert!(
    node
      .client
      .start_run(&node.session_id, "explicit next run", &agent_core::new_id())
      .await
      .is_err()
  );
  // Even available synthetic replies must not cause automatic replay or input adoption.
  classifier
    .push_reply(text_reply(protocol, "invalid decision"))
    .await;
  main
    .push_reply(text_reply(protocol, "must not be requested"))
    .await;
  node.daemon.recover().await.unwrap();
  assert_eq!(
    node
      .daemon
      .store
      .load_snapshot(&node.session_id)
      .await
      .unwrap(),
    Some(before)
  );
  assert_eq!(
    node
      .daemon
      .store
      .runs(&node.session_id)
      .await
      .unwrap()
      .len(),
    1
  );
  let after = node.client.list_run_inputs(&first.id).await.unwrap();
  assert_eq!(after.len(), 1);
  assert_eq!(after[0].entry_id, receipt.entry_id);
  assert_eq!(after[0].state, v1::RunInputState::Pending as i32);
  assert_eq!(main.requests().await.len(), 1);
  assert_eq!(classifier.requests().await.len(), 1);
  assert_eq!(std::fs::read_dir(node.workspace.path()).unwrap().count(), 0);
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_snapshot_previews_are_utf8_safe_and_pending_bytes_are_bounded() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, "invalid decision")])
    .await
    .unwrap();
  let main = MockModelServer::start_interactive().await.unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let run = node.run("wait for input").await;
  requested(&main, 1).await;
  let receipt = node
    .client
    .submit_run_input(v1::RunInputRequest {
      run_id: run.id.clone(),
      owner_epoch: 1,
      request_id: agent_core::new_id(),
      kind: v1::RunInputKind::FollowUp as i32,
      prompt: "界".repeat(2000),
    })
    .await
    .unwrap();
  assert!(!receipt.prompt_truncated);
  let snapshot = node.client.run_input_snapshot(&run.id).await.unwrap();
  assert_eq!(snapshot.inputs.len(), 1);
  assert!(snapshot.inputs[0].prompt_truncated);
  assert_eq!(snapshot.inputs[0].prompt, "界".repeat(1365));
  let before = node
    .daemon
    .store
    .load_snapshot(&node.session_id)
    .await
    .unwrap();
  assert!(
    node
      .client
      .submit_run_input(v1::RunInputRequest {
        run_id: run.id.clone(),
        owner_epoch: 1,
        request_id: agent_core::new_id(),
        kind: v1::RunInputKind::FollowUp as i32,
        prompt: "x".repeat(1_048_576),
      })
      .await
      .is_err()
  );
  assert_eq!(
    node
      .daemon
      .store
      .load_snapshot(&node.session_id)
      .await
      .unwrap(),
    before
  );
  node
    .client
    .cancel_run_input(&run.id, 1, &receipt.entry_id)
    .await
    .unwrap();
  node.client.cancel_run(&run.id).await.unwrap();
  assert_eq!(node.terminal(&run.id).await.status, "uncertain");
  assert_eq!(main.requests().await.len(), 1);
  assert_eq!(std::fs::read_dir(node.workspace.path()).unwrap().count(), 0);
  node.close().await;
}
