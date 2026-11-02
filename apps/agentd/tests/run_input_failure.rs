mod support;
use agent_core::events::{AgentEvent, DecodedEvent, QueueStatus, decode};
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use sea_orm::{ConnectionTrait, Database};
use std::time::Duration;
use support::TestNode;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_queue_cancellation_retains_intent_and_fences_execution() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, "invalid decision")])
    .await
    .unwrap();
  let main = MockModelServer::start_interactive().await.unwrap();
  let node = TestNode::start(&main, &classifier, protocol).await;
  let run = node.run("wait while an input is withdrawn").await;
  tokio::time::timeout(Duration::from_secs(5), main.wait_for_requests(1))
    .await
    .unwrap();
  let receipt = node
    .client
    .submit_run_input(v1::RunInputRequest {
      run_id: run.id.clone(),
      owner_epoch: 1,
      request_id: agent_core::new_id(),
      kind: v1::RunInputKind::FollowUp as i32,
      prompt: "must not be blindly replayed".into(),
    })
    .await
    .unwrap();
  let before = node
    .daemon
    .store
    .load_snapshot(&node.session_id)
    .await
    .unwrap();
  let path = node
    .daemon
    .config
    .data_path()
    .unwrap()
    .join("agent.sqlite3");
  let database = Database::connect(format!("sqlite://{}?mode=rw", path.display()))
    .await
    .unwrap();
  // Reviewed test-only fault injection: reject snapshot writes, while allowing
  // the host intent and Run/Outbox fence to commit through the real Store.
  database.execute_unprepared("CREATE TRIGGER refuse_input_snapshot BEFORE UPDATE ON harness_sessions BEGIN SELECT RAISE(ABORT, 'synthetic snapshot failure'); END;").await.unwrap();
  assert!(
    node
      .client
      .cancel_run_input(&run.id, 1, &receipt.entry_id)
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
  assert_eq!(node.terminal(&run.id).await.status, "uncertain");
  let inputs = node.client.list_run_inputs(&run.id).await.unwrap();
  assert_eq!(inputs[0].state, v1::RunInputState::CancelRequested as i32);
  let events = node.daemon.store.events(&run.id, 0, 1000).await.unwrap();
  let statuses: Vec<_> = events
    .iter()
    .filter_map(
      |event| match decode(&event.kind, &event.payload_json).ok()? {
        DecodedEvent::Known(AgentEvent::QueueUpdated {
          entry_id, status, ..
        }) if entry_id == receipt.entry_id => Some(status),
        _ => None,
      },
    )
    .collect();
  assert_eq!(
    statuses,
    [QueueStatus::Pending, QueueStatus::CancelRequested]
  );
  assert!(
    node
      .client
      .start_run(
        &node.session_id,
        "new input cannot bypass the fence",
        &agent_core::new_id()
      )
      .await
      .is_err()
  );
  assert_eq!(main.requests().await.len(), 1);
  assert_eq!(std::fs::read_dir(node.workspace.path()).unwrap().count(), 0);
  database
    .execute_unprepared("DROP TRIGGER refuse_input_snapshot;")
    .await
    .unwrap();
  assert_eq!(
    node
      .daemon
      .store
      .recover_input_cancellations()
      .await
      .unwrap(),
    0
  );
  node.close().await;
}
