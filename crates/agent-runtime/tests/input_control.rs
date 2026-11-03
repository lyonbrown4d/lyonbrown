#[allow(dead_code)]
mod support;

use std::{
  sync::{
    Arc,
    atomic::{AtomicU8, AtomicUsize, Ordering},
  },
  time::Duration,
};

use agent_core::events::{AgentEvent, QueueKind, QueueStatus};
use agent_runtime::{Result, Runtime, RuntimeError, SessionMutation, SessionPersistence};
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol};

#[derive(Default)]
struct QueueFault {
  inner: support::Memory,
  reject: AtomicU8,
  failures: AtomicUsize,
}

#[async_trait::async_trait]
impl SessionPersistence for QueueFault {
  async fn load(&self, session: &str) -> Result<Option<Vec<u8>>> {
    self.inner.load(session).await
  }

  async fn commit(
    &self,
    session: &str,
    epoch: u64,
    snapshot: Vec<u8>,
    mutation: SessionMutation,
  ) -> Result<()> {
    let reject = self.reject.load(Ordering::Acquire);
    let denied = mutation.events.iter().any(|event| {
      matches!(event, AgentEvent::QueueUpdated { status, .. }
        if (reject == 1 && *status == QueueStatus::Pending)
          || (reject == 2 && *status == QueueStatus::Cancelled))
    });
    if denied {
      self.failures.fetch_add(1, Ordering::SeqCst);
      return Err(RuntimeError::Persistence(
        "synthetic queue commit refusal".into(),
      ));
    }
    self.inner.commit(session, epoch, snapshot, mutation).await
  }
}

async fn failure_fences_execution(cancel: bool) {
  for protocol in StandardProtocol::ALL {
    let server = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let store = Arc::new(QueueFault::default());
    let mut config = support::config(server.base_url());
    config.provider = protocol.provider().into();
    let runtime = Arc::new(
      Runtime::open(config, store.clone(), vec![], Arc::new(|_| {}))
        .await
        .unwrap(),
    );
    let execution = {
      let runtime = runtime.clone();
      tokio::spawn(async move {
        runtime
          .execute("keep the initial model response open")
          .await
      })
    };
    tokio::time::timeout(Duration::from_secs(5), server.wait_for_requests(1))
      .await
      .unwrap();
    let entry = if cancel {
      Some(
        runtime
          .enqueue_input("accepted-request", QueueKind::FollowUp, "durable follow-up")
          .await
          .unwrap(),
      )
    } else {
      None
    };
    store
      .reject
      .store(if cancel { 2 } else { 1 }, Ordering::Release);
    let error = if let Some(entry) = &entry {
      runtime.cancel_input(entry).await.unwrap_err()
    } else {
      runtime
        .enqueue_input(
          "refused-request",
          QueueKind::Steer,
          "must never be accepted",
        )
        .await
        .unwrap_err()
    };
    assert!(
      matches!(error, RuntimeError::Uncertain(_)),
      "{protocol:?}: {error}"
    );
    assert!(runtime.input_persistence_failed());
    // Returning Uncertain is insufficient if the model execution keeps running.
    let result = tokio::time::timeout(Duration::from_secs(5), execution)
      .await
      .unwrap()
      .unwrap();
    assert!(
      matches!(result, Err(RuntimeError::Uncertain(_))),
      "{protocol:?}: {result:?}"
    );
    assert!(
      runtime
        .enqueue_input("later-request", QueueKind::FollowUp, "must remain a draft")
        .await
        .is_err()
    );
    assert_eq!(store.failures.load(Ordering::SeqCst), 1);
    assert_eq!(server.requests().await.len(), 1);
    let mutations = store.inner.mutations.lock().unwrap();
    let queue: Vec<_> = mutations
      .iter()
      .flat_map(|mutation| &mutation.events)
      .filter_map(|event| match event {
        AgentEvent::QueueUpdated {
          entry_id,
          status,
          request_id,
          ..
        } => Some((entry_id.clone(), *status, request_id.clone())),
        _ => None,
      })
      .collect();
    if let Some(entry) = entry {
      // Copy-on-commit keeps the in-memory item after its refused cancellation.
      // The product must retain the last durable Pending fact, never acknowledge
      // a cancellation or consumption that did not commit.
      assert_eq!(
        queue,
        vec![(
          entry.clone(),
          QueueStatus::Pending,
          Some("accepted-request".into())
        )]
      );
      let snapshot = store.inner.snapshot.lock().unwrap();
      assert!(
        String::from_utf8(snapshot.clone().unwrap())
          .unwrap()
          .contains(&entry)
      );
    } else {
      assert!(queue.is_empty());
      let snapshot = store.inner.snapshot.lock().unwrap();
      assert!(
        !String::from_utf8(snapshot.clone().unwrap())
          .unwrap()
          .contains("must never be accepted")
      );
    }
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enqueue_commit_failure_aborts_real_runtime_and_closes_admission() {
  failure_fences_execution(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_commit_failure_aborts_real_runtime_and_keeps_pending_receipt() {
  failure_fences_execution(true).await;
}
