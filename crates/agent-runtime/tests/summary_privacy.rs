#[allow(dead_code)]
mod support;
use agent_runtime::{Runtime, SummaryInput, SummaryRequestGuard};
use agent_test_support::MockModelServer;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn canonical_summary_privacy_is_available_without_an_extra_host_guard() {
  let server = MockModelServer::start(vec![
    support::text("one"),
    support::text("two"),
    support::text("safe checkpoint"),
  ])
  .await
  .unwrap();
  let runtime = Runtime::open(
    support::config(server.base_url()),
    support::shared(),
    vec![],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  runtime.execute("task").await.unwrap();
  runtime.execute("followup").await.unwrap();
  assert!(
    runtime
      .context_status()
      .await
      .unwrap()
      .summary_pipeline_available
  );
  let after = runtime.compact("").await.unwrap();
  assert!(after.has_summary && !after.summary_request_pending);
  assert_eq!(server.requests().await.len(), 3);
}

struct DenySummary;
#[async_trait::async_trait]
impl SummaryRequestGuard for DenySummary {
  async fn prepare(
    &self,
    _: SummaryInput,
    _: CancellationToken,
  ) -> agent_runtime::Result<SummaryInput> {
    Err(agent_runtime::RuntimeError::Configuration(
      "additional host policy denied summary".into(),
    ))
  }
}
#[tokio::test]
async fn additional_host_policy_can_still_refuse_summary_io_without_changing_context() {
  let server = MockModelServer::start(vec![support::text("one"), support::text("two")])
    .await
    .unwrap();
  let runtime = Runtime::open(
    support::config(server.base_url()),
    support::shared(),
    vec![],
    Arc::new(|_| {}),
  )
  .await
  .unwrap()
  .with_summary_guard(Arc::new(DenySummary));
  runtime.execute("task").await.unwrap();
  runtime.execute("followup").await.unwrap();
  let before = runtime.context_status().await.unwrap();
  assert!(runtime.compact("").await.is_err());
  let after = runtime.context_status().await.unwrap();
  assert_eq!(after.message_count, before.message_count);
  assert_eq!(after.revision, before.revision);
  assert!(!after.has_summary && !after.summary_request_pending);
  assert_eq!(server.requests().await.len(), 2);
}

struct SensitiveHostInput;
#[async_trait::async_trait]
impl SummaryRequestGuard for SensitiveHostInput {
  async fn prepare(
    &self,
    mut input: SummaryInput,
    _: CancellationToken,
  ) -> agent_runtime::Result<SummaryInput> {
    input
      .system_prompt
      .push_str(" password=synthetic-summary-secret");
    input.transcript.push_str(" person@example.test");
    Ok(input)
  }
}
#[tokio::test]
async fn extra_host_policy_cannot_bypass_canonical_summary_input_protection() {
  let server = MockModelServer::start(vec![
    support::text("one"),
    support::text("two"),
    support::text("safe checkpoint"),
  ])
  .await
  .unwrap();
  let runtime = Runtime::open(
    support::config(server.base_url()),
    support::shared(),
    vec![],
    Arc::new(|_| {}),
  )
  .await
  .unwrap()
  .with_summary_guard(Arc::new(SensitiveHostInput));
  runtime.execute("task").await.unwrap();
  runtime.execute("followup").await.unwrap();
  runtime.compact("").await.unwrap();
  let requests = server.requests().await;
  let request = requests[2].body.to_string();
  assert!(!request.contains("synthetic-summary-secret"));
  assert!(!request.contains("person@example.test"));
  assert!(request.contains("[PRIVATE:"));
}

#[tokio::test]
async fn sensitive_summary_output_is_rejected_without_replacing_context() {
  let server = MockModelServer::start(vec![
    support::text("one"),
    support::text("two"),
    support::text("fixture-secret-never-persist"),
  ])
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
  runtime.execute("task").await.unwrap();
  runtime.execute("followup").await.unwrap();
  let before = runtime.context_status().await.unwrap();
  assert!(runtime.compact("").await.is_err());
  let after = runtime.context_status().await.unwrap();
  assert_eq!(after.revision, before.revision);
  assert!(!after.has_summary && !after.summary_request_pending);
  let saved = store.snapshot.lock().unwrap().clone().unwrap();
  assert!(
    !String::from_utf8(saved)
      .unwrap()
      .contains("fixture-secret-never-persist")
  );
}
