#[allow(dead_code)]
mod support;
use agent_core::events::QueueKind;
use agent_runtime::{Runtime, SummaryInput, SummaryRequestGuard};
use agent_test_support::MockModelServer;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
struct Guard;
#[async_trait::async_trait]
impl SummaryRequestGuard for Guard {
  async fn prepare(
    &self,
    input: SummaryInput,
    _: CancellationToken,
  ) -> agent_runtime::Result<SummaryInput> {
    Ok(input)
  }
}
#[tokio::test]
async fn steering_and_followup_constraints_survive_unfaithful_summary_undo_and_new_run_once() {
  let server = MockModelServer::start(vec![
    support::text("old output"),
    support::text("recent output"),
    support::text("checkpoint deliberately omits every user constraint"),
    support::text("continued"),
    support::text("after undo"),
  ])
  .await
  .unwrap();
  let config = support::config(server.base_url());
  let store = support::shared();
  let runtime = Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap()
    .with_summary_guard(Arc::new(Guard));
  runtime
    .enqueue_input(
      "steer",
      QueueKind::Steer,
      "STEER_KEEP: never modify production",
    )
    .await
    .unwrap();
  runtime
    .execute("ROOT_KEEP: use the synthetic workspace")
    .await
    .unwrap();
  runtime
    .execute("FOLLOW_KEEP: preserve the account A restriction")
    .await
    .unwrap();
  runtime.compact("").await.unwrap();
  runtime
    .execute("continue under those exact constraints")
    .await
    .unwrap();
  let projected = server.requests().await[3].body.to_string();
  for marker in ["ROOT_KEEP", "STEER_KEEP", "FOLLOW_KEEP"] {
    assert_eq!(projected.matches(marker).count(), 1, "{marker}");
  }
  drop(runtime);
  let runtime = Runtime::open(config, store, vec![], Arc::new(|_| {}))
    .await
    .unwrap();
  runtime.undo_compaction().await.unwrap();
  runtime.execute("continue after undo").await.unwrap();
  let after_undo = server.requests().await[4].body.to_string();
  for marker in ["ROOT_KEEP", "STEER_KEEP", "FOLLOW_KEEP"] {
    assert_eq!(after_undo.matches(marker).count(), 1, "{marker}");
  }
  assert!(!after_undo.contains("checkpoint deliberately omits"));
}
