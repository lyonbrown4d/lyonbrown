//! Restore the exact product snapshot through both Runtime and public SDK APIs.
#[allow(dead_code)]
mod support;

use std::{sync::Arc, time::Duration};

use agent_core::events::{AgentEvent, QueueKind, QueueStatus};
use agent_runtime::{RunOutcome, Runtime};
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol};
use rpi_harness::{
  agent_harness::{AgentHarness, AgentLane, CancelQueuedOutcome},
  session::{
    DefaultIdGenerator, Session,
    memory::{InMemorySessionStorage, SystemClock},
    state::SessionState,
    types::{LaneRecord, RecordQuery, SessionMetadata},
  },
  types::AgentHarnessOptions,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Snapshot {
  metadata: SessionMetadata,
  state: SessionState,
}

async fn wait_model(model: &MockModelServer) {
  tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(1))
    .await
    .unwrap();
}

async fn open(
  protocol: StandardProtocol,
  model: &MockModelServer,
  memory: Arc<support::Memory>,
) -> Arc<Runtime> {
  let mut config = support::config(model.base_url());
  config.provider = protocol.provider().into();
  Arc::new(
    Runtime::open(config, memory, vec![], Arc::new(|_| {}))
      .await
      .unwrap(),
  )
}

fn queue_statuses(memory: &support::Memory, id: &str) -> Vec<QueueStatus> {
  memory
    .mutations
    .lock()
    .unwrap()
    .iter()
    .flat_map(|mutation| &mutation.events)
    .filter_map(|event| match event {
      AgentEvent::QueueUpdated {
        entry_id, status, ..
      } if entry_id == id => Some(*status),
      _ => None,
    })
    .collect()
}

async fn restoration(cancel_during_next_execution: bool) {
  for protocol in StandardProtocol::ALL {
    let first_model = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let memory = support::shared();
    let first = open(protocol, &first_model, memory.clone()).await;
    let execution = {
      let first = first.clone();
      tokio::spawn(async move { first.execute("initial prompt").await })
    };
    wait_model(&first_model).await;
    let prompt = "queued follow-up must survive the cancelled model response";
    let entry = first
      .enqueue_input("stable-request", QueueKind::FollowUp, prompt)
      .await
      .unwrap();
    first.cancel().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), execution)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    assert_eq!(result.outcome, RunOutcome::Cancelled);
    assert!(first.has_pending_inputs().await.unwrap());
    assert!(!first.input_consumed(&entry).await.unwrap());
    assert_eq!(queue_statuses(&memory, &entry), [QueueStatus::Pending]);
    drop(first);
    let encoded = memory.snapshot.lock().unwrap().clone().unwrap();
    let snapshot: Snapshot = serde_json::from_slice(&encoded).unwrap();
    let records = snapshot
      .state
      .find_records(&RecordQuery {
        lane: Some("main".into()),
        ..Default::default()
      })
      .unwrap();
    let enqueued: Vec<_> = records
      .iter()
      .filter_map(|record| match record {
        LaneRecord::QueueEnqueued(record) if record.target["id"] == entry => Some(record),
        _ => None,
      })
      .collect();
    assert_eq!(
      enqueued.len(),
      1,
      "serialized snapshot must retain original queue intent"
    );
    assert!(snapshot.state.get_entry(&entry).is_none());
    assert!(!records.iter().any(
      |record| matches!(record, LaneRecord::QueueCancelled(record) if record.entry_id == entry)
    ));
    let target = enqueued[0].target.clone();
    // An independent public-SDK restore of the exact serialized SessionState
    // distinguishes a product persistence defect from SDK queue reconstruction.
    let storage = Arc::new(InMemorySessionStorage::from_state(
      snapshot.metadata,
      snapshot.state,
      Arc::new(SystemClock),
      Arc::new(DefaultIdGenerator::new()),
    ));
    let sdk = AgentHarness::create(AgentHarnessOptions {
      session: Session::new(storage, None),
      allow_existing_session: true,
      models: vec![],
      ..Default::default()
    })
    .await
    .unwrap();
    let sdk_queue = sdk.queued_messages().await.unwrap();
    let sdk_cancel = sdk.cancel_queued(&entry).await.unwrap();
    // Use a separate model server for Runtime 2: a reply released after an
    // aborted request must never accidentally complete the next model turn.
    let next_model = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let next = open(protocol, &next_model, memory.clone()).await;
    let restored_pending = next.has_pending_inputs().await.unwrap();
    let execution = if cancel_during_next_execution {
      let next = next.clone();
      let execution = tokio::spawn(async move { next.execute("explicit next prompt").await });
      wait_model(&next_model).await;
      Some(execution)
    } else {
      None
    };
    let before_cancel_pending = next.has_pending_inputs().await.unwrap();
    let consumed = next.input_consumed(&entry).await.unwrap();
    let cancelled = next.cancel_input(&entry).await.unwrap();
    if let Some(execution) = execution {
      next.cancel().await.unwrap();
      let result = tokio::time::timeout(Duration::from_secs(5), execution)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
      assert_eq!(result.outcome, RunOutcome::Cancelled);
    }
    assert_eq!(
      sdk_queue.follow_up,
      [prompt],
      "{protocol:?}; serialized target: {target}"
    );
    assert_eq!(
      sdk_cancel.outcome,
      CancelQueuedOutcome::Cancelled,
      "{protocol:?}; target: {target}"
    );
    assert!(
      restored_pending,
      "{protocol:?}: Runtime::open did not rebuild the durable queue; target: {target}"
    );
    assert!(
      before_cancel_pending,
      "{protocol:?}: queued follow-up disappeared before the next response completed"
    );
    assert!(
      !consumed,
      "{protocol:?}: original reserved entry was unexpectedly consumed"
    );
    assert!(
      cancelled,
      "{protocol:?}: cancellation returned AlreadyConsumed without a durable entry; target: {target}"
    );
    assert!(!next.has_pending_inputs().await.unwrap());
    assert_eq!(
      queue_statuses(&memory, &entry),
      [QueueStatus::Pending, QueueStatus::Cancelled]
    );
    assert_eq!(first_model.requests().await.len(), 1);
    assert_eq!(
      next_model.requests().await.len(),
      usize::from(cancel_during_next_execution)
    );
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_runtime_restores_pending_follow_up_with_its_original_entry_identity() {
  restoration(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restored_follow_up_remains_cancellable_while_the_next_model_response_is_open() {
  restoration(true).await;
}
