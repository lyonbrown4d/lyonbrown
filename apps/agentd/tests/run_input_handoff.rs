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
async fn inherited_input_cannot_be_cancelled_through_a_stale_runtime_during_handoff() {
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
      prompt: "retained input must be cancellable".into(),
    })
    .await
    .unwrap();
  node.client.cancel_run(&first.id).await.unwrap();
  assert_eq!(node.terminal(&first.id).await.status, "cancelled");
  let next = node.run("explicit next run").await;
  requested(&classifier, 2).await;
  let before = node
    .daemon
    .store
    .load_snapshot(&node.session_id)
    .await
    .unwrap();
  assert!(
    node
      .client
      .cancel_run_input(&first.id, 1, &receipt.entry_id)
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
  let inherited = node.client.list_run_inputs(&next.id).await.unwrap();
  assert_eq!(inherited.len(), 1);
  assert_eq!(inherited[0].run_id, first.id);
  assert_eq!(inherited[0].state, v1::RunInputState::Pending as i32);
  classifier
    .push_reply(text_reply(protocol, "invalid decision"))
    .await;
  requested(&main, 2).await;
  let cancelled = node
    .client
    .cancel_run_input(&first.id, 1, &receipt.entry_id)
    .await
    .unwrap();
  assert!(matches!(
    v1::RunInputState::try_from(cancelled.state).unwrap(),
    v1::RunInputState::Cancelled | v1::RunInputState::Consumed
  ));
  let adopted = main.requests().await[1]
    .body
    .to_string()
    .contains("retained input must be cancellable");
  assert_eq!(
    adopted,
    cancelled.state == v1::RunInputState::Consumed as i32,
    "consumption must correspond to an actual model request; otherwise withdrawal must be durable"
  );
  main
    .push_reply(text_reply(protocol, "next run completed"))
    .await;
  assert_eq!(node.terminal(&next.id).await.status, "completed");
  assert_eq!(
    main.requests().await.len(),
    2,
    "withdrawn inherited input must never request another turn"
  );
  assert_eq!(
    node.client.list_run_inputs(&next.id).await.unwrap()[0].state,
    cancelled.state
  );
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
  assert_eq!(node.terminal(&run.id).await.status, "cancelled");
  assert_eq!(main.requests().await.len(), 1);
  assert_eq!(std::fs::read_dir(node.workspace.path()).unwrap().count(), 0);
  node.close().await;
}
