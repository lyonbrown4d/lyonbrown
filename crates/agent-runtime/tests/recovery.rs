#[allow(dead_code)]
mod support;
use agent_runtime::{Runtime, RuntimeError};
use agent_test_support::{MockModelServer, ScriptedReply};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn interrupted_provider_intent_is_inspectable_and_never_reissued_on_open() {
  let model = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let store = support::shared();
  let config = support::config(model.base_url());
  let runtime = Arc::new(
    Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
      .await
      .unwrap(),
  );
  let execution = {
    let runtime = runtime.clone();
    tokio::spawn(async move { runtime.execute("provider pending").await })
  };
  tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(1))
    .await
    .unwrap();
  execution.abort();
  let _ = execution.await;
  let before = store.snapshot.lock().unwrap().clone();
  let count = store.mutations.lock().unwrap().len();
  let report = agent_runtime::inspect_recovery("session", 1, store.clone())
    .await
    .unwrap();
  assert!(report.pending_tools.is_empty());
  assert!(report.uncertainty.unwrap().contains("provider request"));
  let reopened = Runtime::open(config, store.clone(), vec![], Arc::new(|_| {})).await;
  assert!(matches!(reopened, Err(RuntimeError::Uncertain(_))));
  assert_eq!(*store.snapshot.lock().unwrap(), before);
  assert_eq!(store.mutations.lock().unwrap().len(), count);
  assert_eq!(model.requests().await.len(), 1);
}
#[tokio::test]
async fn stale_owner_is_fenced_before_any_provider_request() {
  let model = MockModelServer::start(vec![support::text("must not be requested")])
    .await
    .unwrap();
  let mut config = support::config(model.base_url());
  config.owner_epoch = 2;
  let store = support::shared();
  let runtime = Runtime::open(config, store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap();
  assert!(matches!(
    runtime.execute("stale owner").await,
    Err(RuntimeError::Uncertain(_))
  ));
  assert!(model.requests().await.is_empty());
  assert!(store.snapshot.lock().unwrap().is_none());
}
