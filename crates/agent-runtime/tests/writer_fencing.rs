mod support;
use agent_core::events::QueueKind;
use agent_runtime::{Result, Runtime, RuntimeError, SessionMutation, SessionPersistence};
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
struct RefuseFirst {
  attempts: AtomicUsize,
  release: Notify,
}
#[async_trait::async_trait]
impl SessionPersistence for RefuseFirst {
  async fn load(&self, _: &str) -> Result<Option<Vec<u8>>> {
    Ok(None)
  }
  async fn commit(&self, _: &str, _: u64, _: Vec<u8>, _: SessionMutation) -> Result<()> {
    self.attempts.fetch_add(1, Ordering::SeqCst);
    self.release.notified().await;
    Err(RuntimeError::Persistence("synthetic write refusal".into()))
  }
}

#[tokio::test]
async fn a_queued_writer_cannot_cross_a_failed_commit_fence() {
  let store = Arc::new(RefuseFirst::default());
  let runtime = Runtime::open(
    support::config("http://127.0.0.1:1"),
    store.clone(),
    vec![],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  let first = runtime.enqueue_input("first", QueueKind::Steer, "first fixture");
  tokio::pin!(first);
  assert!(futures::poll!(first.as_mut()).is_pending());
  assert_eq!(store.attempts.load(Ordering::SeqCst), 1);
  let second = runtime.enqueue_input("second", QueueKind::Steer, "second fixture");
  tokio::pin!(second);
  // Polling proves this caller has reached the locked state before failure.
  assert!(futures::poll!(second.as_mut()).is_pending());
  store.release.notify_one();
  assert!(matches!(first.await, Err(RuntimeError::Uncertain(_))));
  assert!(matches!(second.await, Err(RuntimeError::Uncertain(_))));
  assert_eq!(store.attempts.load(Ordering::SeqCst), 1);
  assert!(!runtime.has_pending_inputs().await.unwrap());
}

#[tokio::test]
async fn cancellation_before_execution_is_polled_sends_no_model_request() {
  let server = agent_test_support::MockModelServer::start(vec![support::text("must not execute")])
    .await
    .unwrap();
  let store = support::shared();
  let runtime = Runtime::open(
    support::config(server.base_url()),
    store.clone(),
    vec![],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  runtime.cancel().await.unwrap();
  let result = runtime.execute("cancelled fixture").await.unwrap();
  assert_eq!(result.outcome, agent_runtime::RunOutcome::Cancelled);
  assert!(server.requests().await.is_empty());
  let snapshot: serde_json::Value =
    serde_json::from_slice(store.snapshot.lock().unwrap().as_ref().unwrap()).unwrap();
  assert!(snapshot["active"].is_null());
  assert_eq!(snapshot["pending_tools"], serde_json::json!({}));
}
