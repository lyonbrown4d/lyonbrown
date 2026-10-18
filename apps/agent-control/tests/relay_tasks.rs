mod relay_task_support;

use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use relay_task_support::{RelayPair, worker_session};
use serde_json::json;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outbound_daemons_execute_and_accept_harness_tasks_over_relay_for_every_protocol() {
  for protocol in StandardProtocol::ALL {
    let model = MockModelServer::start(vec![
      tool_reply(protocol, "read_file", json!({"path":"master-only.txt"})),
      text_reply(protocol, "private-worker-result-over-relay"),
    ])
    .await
    .unwrap();
    let mut pair = RelayPair::start(&model, protocol).await;
    pair.assert_direct_unreachable().await;
    let envelope = pair.envelope();
    let submitted = pair
      .dispatch
      .submit(&pair.worker_node(), envelope.clone())
      .await
      .unwrap();
    assert_eq!(submitted.envelope.unwrap().task_id, envelope.task_id);
    let task = pair.terminal(&envelope.task_id).await;
    assert_eq!(task.status, "awaiting_acceptance");
    assert_eq!(task.attempt_status, "succeeded", "{}", task.error);
    assert_eq!(task.result, "private-worker-result-over-relay");
    let worker = pair.worker.store.task(&envelope.task_id).unwrap().unwrap();
    assert_eq!(worker.status, "awaiting_acceptance");
    assert_eq!(worker.envelope, envelope);
    let session = pair
      .worker
      .store
      .session(&worker_session(&envelope))
      .unwrap()
      .unwrap();
    assert_ne!(session.id, pair.session.id);
    assert_ne!(session.workspace, envelope.workspace);
    assert!(
      !std::path::Path::new(&session.workspace)
        .join("master-only.txt")
        .exists()
    );
    let runs = pair.worker.store.runs(&session.id).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, "completed");
    assert_eq!(runs[0].result, task.result);
    let events = pair.worker.store.events(&runs[0].id, 0, 1000).unwrap();
    assert!(events.iter().any(|event| event.kind == "run_started"));
    let plan: serde_json::Value = serde_json::from_str(
      &events
        .iter()
        .find(|event| event.kind == "execution_plan")
        .unwrap()
        .payload_json,
    )
    .unwrap();
    assert_eq!(plan["allowed_tools"], json!(["read_file"]));
    let requests = model.requests().await;
    assert_eq!(requests.len(), 2);
    assert!(
      requests
        .iter()
        .all(|request| request.path == protocol.path())
    );
    assert!(
      !requests[1]
        .body
        .to_string()
        .contains("master-private-relay-content")
    );
    assert!(
      pair
        .dispatch
        .submit(&pair.worker_node(), envelope.clone())
        .await
        .is_err()
    );
    assert_eq!(pair.worker.store.runs(&session.id).unwrap().len(), 1);
    let accepted = pair
      .dispatch
      .accept(&pair.worker_node(), &envelope.task_id, true)
      .await
      .unwrap();
    assert_eq!(accepted.status, "completed");
    assert_eq!(
      pair
        .worker
        .store
        .task(&envelope.task_id)
        .unwrap()
        .unwrap()
        .status,
      "completed"
    );
    pair.control.assert_no_task_plaintext(&envelope);
    eprintln!(
      "{protocol:?}: two outbound daemons, relayed mTLS, actual Worker Harness, isolated workspace and Master acceptance verified"
    );
    pair.stop().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_disconnect_after_cancel_intent_is_uncertain_until_same_attempt_reconciles() {
  let protocol = StandardProtocol::ChatCompletions;
  let model = MockModelServer::start_interactive().await.unwrap();
  let mut pair = RelayPair::start(&model, protocol).await;
  pair.assert_direct_unreachable().await;
  let envelope = pair.envelope();
  pair
    .dispatch
    .submit(&pair.worker_node(), envelope.clone())
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(1))
    .await
    .unwrap();
  let worker_before = pair.worker.store.task(&envelope.task_id).unwrap().unwrap();
  assert_eq!(worker_before.attempt_status, "running");
  pair.control.disconnect().await;
  assert!(
    pair
      .dispatch
      .cancel(&pair.worker_node(), &envelope.task_id)
      .await
      .is_err()
  );
  let unknown = pair.master.store.task(&envelope.task_id).unwrap().unwrap();
  assert_eq!(unknown.attempt_status, "uncertain");
  assert_eq!(unknown.envelope, envelope);
  assert_eq!(
    pair
      .worker
      .store
      .task(&envelope.task_id)
      .unwrap()
      .unwrap()
      .attempt_status,
    "running"
  );
  assert!(
    pair
      .dispatch
      .submit(&pair.worker_node(), envelope.clone())
      .await
      .is_err()
  );
  assert_eq!(
    model.requests().await.len(),
    1,
    "disconnect must not replay model execution"
  );
  pair.control.reconnect().await;
  pair.wait_connected().await;
  assert!(
    pair
      .dispatch
      .submit(&pair.worker_node(), envelope.clone())
      .await
      .is_err()
  );
  assert_eq!(model.requests().await.len(), 1);
  let still_unknown = pair.master.store.task(&envelope.task_id).unwrap().unwrap();
  assert_eq!(
    still_unknown.attempt_status, "uncertain",
    "transport reconnect does not reconcile business state automatically"
  );
  model
    .push_reply(text_reply(protocol, "private-worker-result-over-relay"))
    .await;
  tokio::time::timeout(Duration::from_secs(10), async {
    while pair
      .worker
      .store
      .task(&envelope.task_id)
      .unwrap()
      .unwrap()
      .attempt_status
      != "succeeded"
    {
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  })
  .await
  .unwrap();
  assert_eq!(
    pair
      .master
      .store
      .task(&envelope.task_id)
      .unwrap()
      .unwrap()
      .attempt_status,
    "uncertain"
  );
  let reconciled = pair.terminal(&envelope.task_id).await;
  assert_eq!(reconciled.envelope, envelope);
  assert_eq!(reconciled.attempt_status, "succeeded");
  assert_eq!(reconciled.status, "awaiting_acceptance");
  assert_eq!(reconciled.result, "private-worker-result-over-relay");
  let accepted = pair
    .dispatch
    .accept(&pair.worker_node(), &envelope.task_id, true)
    .await
    .unwrap();
  assert_eq!(accepted.status, "completed");
  assert_eq!(
    pair
      .worker
      .store
      .runs(&worker_session(&envelope))
      .unwrap()
      .len(),
    1
  );
  assert_eq!(model.requests().await.len(), 1);
  pair.control.assert_no_task_plaintext(&envelope);
  eprintln!(
    "relay disconnect: durable cancellation intent -> Master Uncertain -> same task/attempt reconcile after reconnect, without replay"
  );
  pair.stop().await;
}
