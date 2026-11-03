//! Product-owned queue identities survive Rig run cancellation and restart.
#[allow(dead_code)]
mod support;
use agent_core::events::{AgentEvent, QueueKind, QueueStatus};
use agent_runtime::{RunOutcome, Runtime};
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn cancelled_run_preserves_pending_input_and_its_cancellation_receipt() {
  for protocol in StandardProtocol::ALL {
    let model = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let memory = support::shared();
    let mut config = support::config(model.base_url());
    config.provider = protocol.provider().into();
    let runtime = Arc::new(
      Runtime::open(config.clone(), memory.clone(), vec![], Arc::new(|_| {}))
        .await
        .unwrap(),
    );
    let execution = {
      let runtime = runtime.clone();
      tokio::spawn(async move { runtime.execute("initial prompt").await })
    };
    tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(1))
      .await
      .unwrap();
    let id = runtime
      .enqueue_input("stable-request", QueueKind::FollowUp, "durable follow-up")
      .await
      .unwrap();
    runtime.cancel().await.unwrap();
    assert_eq!(
      execution.await.unwrap().unwrap().outcome,
      RunOutcome::Cancelled
    );
    assert!(runtime.has_pending_inputs().await.unwrap());
    assert!(!runtime.input_consumed(&id).await.unwrap());
    drop(runtime);
    let before = memory.snapshot.lock().unwrap().clone();
    let next = Runtime::open(config, memory.clone(), vec![], Arc::new(|_| {}))
      .await
      .unwrap();
    assert_eq!(
      *memory.snapshot.lock().unwrap(),
      before,
      "opening must be read only"
    );
    assert!(next.has_pending_inputs().await.unwrap());
    assert!(next.cancel_input(&id).await.unwrap());
    assert!(!next.cancel_input(&id).await.unwrap());
    assert!(!next.has_pending_inputs().await.unwrap());
    assert!(!next.input_consumed(&id).await.unwrap());
    assert_eq!(model.requests().await.len(), 1);
    let statuses: Vec<_> = memory
      .mutations
      .lock()
      .unwrap()
      .iter()
      .flat_map(|mutation| &mutation.events)
      .filter_map(|event| match event {
        AgentEvent::QueueUpdated {
          entry_id, status, ..
        } if entry_id == &id => Some(*status),
        _ => None,
      })
      .collect();
    assert_eq!(statuses, [QueueStatus::Pending, QueueStatus::Cancelled]);
  }
}
