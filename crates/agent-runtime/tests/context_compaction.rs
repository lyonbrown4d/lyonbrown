#[allow(dead_code)]
mod support;
use agent_runtime::{Runtime, SummaryInput, SummaryRequestGuard};
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Explicit host policy for synthetic fixture content, not a production default.
struct FixtureGuard;
#[async_trait::async_trait]
impl SummaryRequestGuard for FixtureGuard {
  async fn prepare(
    &self,
    input: SummaryInput,
    _: CancellationToken,
  ) -> agent_runtime::Result<SummaryInput> {
    Ok(SummaryInput {
      system_prompt: input.system_prompt,
      transcript: input.transcript.replace("private-fixture", "[REDACTED]"),
    })
  }
}

#[tokio::test]
async fn manual_compaction_keeps_task_and_system_then_undo_restores_after_reopen() {
  let mut server = support::Server::start(vec![
    support::text("old private-fixture finding"),
    support::text("recent exact finding"),
    support::text("checkpoint [REDACTED]"),
    support::text("continued"),
  ])
  .await;
  let store = support::shared();
  let config = support::config(&server.endpoint);
  let runtime = Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap()
    .with_summary_guard(Arc::new(FixtureGuard));
  runtime
    .execute("Exact task constraint: never modify account A")
    .await
    .unwrap();
  runtime.execute("continue investigation").await.unwrap();
  let before = runtime.context_status().await.unwrap();
  let compacted = runtime.compact("retain findings").await.unwrap();
  assert!(compacted.has_summary && compacted.can_undo);
  assert!(compacted.revision > before.revision);
  assert!(compacted.estimated_input_tokens > 0);
  for _ in 0..2 {
    server.request().await;
  }
  let request = server.request().await;
  assert!(
    request
      .get("tools")
      .is_none_or(|tools| tools.as_array().unwrap().is_empty())
  );
  let request_text = request.to_string();
  assert!(!request_text.contains("private-fixture"));
  assert!(!request_text.contains("Exact task constraint"));
  assert!(request_text.contains("[REDACTED]"));
  drop(runtime);
  let runtime = Runtime::open(config, store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap()
    .with_summary_guard(Arc::new(FixtureGuard));
  assert!(runtime.context_status().await.unwrap().has_summary);
  let restored = runtime.undo_compaction().await.unwrap();
  assert!(!restored.has_summary && !restored.can_undo);
  assert_eq!(restored.message_count, before.message_count);
  runtime.execute("report after undo").await.unwrap();
  let request = server.request().await.to_string();
  assert!(request.contains("Exact task constraint: never modify account A"));
  assert!(request.contains("Use supplied tools."));
  assert!(request.contains("old private-fixture finding"));
  assert!(request.contains("recent exact finding"));
  assert!(!request.contains("checkpoint [REDACTED]"));
  let mutations = store.mutations.lock().unwrap();
  assert!(
    mutations
      .iter()
      .any(|mutation| mutation.kind == "context_compaction_committed")
  );
  assert!(
    mutations
      .iter()
      .any(|mutation| mutation.kind == "context_compaction_undone")
  );
  assert!(
    !serde_json::to_string(&*mutations)
      .unwrap()
      .contains("private-fixture")
  );
}

#[tokio::test]
async fn near_window_summary_is_automatic_and_next_request_uses_its_projection() {
  let protocol = StandardProtocol::ChatCompletions;
  let long = "verified-output ".repeat(1500);
  let recent = "recent-result ".repeat(1000);
  let server = MockModelServer::start(vec![
    text_reply(protocol, &long),
    text_reply(protocol, &recent),
    text_reply(protocol, "bounded checkpoint"),
    text_reply(protocol, "done"),
  ])
  .await
  .unwrap();
  let mut config = support::config(server.base_url());
  config.context_window = 12_000;
  config.max_output_tokens = 1000;
  let store = support::shared();
  let runtime = Runtime::open(config, store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap()
    .with_summary_guard(Arc::new(FixtureGuard));
  runtime.execute("original immutable task").await.unwrap();
  runtime.execute("inspect second source").await.unwrap();
  runtime.execute("continue").await.unwrap();
  let requests = server.requests().await;
  assert_eq!(requests.len(), 4);
  assert!(
    requests[2]
      .body
      .to_string()
      .contains("structured checkpoint")
  );
  let projected = requests[3].body.to_string();
  assert!(projected.contains("bounded checkpoint"));
  assert!(projected.contains("original immutable task"));
  assert!(!projected.contains(&long));
  let status = runtime.context_status().await.unwrap();
  assert!(status.has_summary);
  assert!(
    status.estimated_input_tokens + u64::from(status.reserved_output_tokens)
      < u64::from(status.context_window)
  );
}

#[tokio::test]
async fn missing_summary_pipeline_refuses_io_without_changing_context() {
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
  .unwrap();
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

#[tokio::test]
async fn forbidden_tool_midstream_keeps_unknown_intent_and_does_not_execute() {
  let server = MockModelServer::start(vec![
    support::text("one"),
    support::text("two"),
    support::tool(),
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
  .unwrap()
  .with_summary_guard(Arc::new(FixtureGuard));
  runtime.execute("task").await.unwrap();
  runtime.execute("followup").await.unwrap();
  let before = runtime.context_status().await.unwrap();
  assert!(matches!(
    runtime.compact("").await,
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
  let after = runtime.context_status().await.unwrap();
  assert_eq!(after.message_count, before.message_count);
  assert_eq!(after.revision, before.revision);
  assert!(!after.has_summary && after.summary_request_pending);
  assert!(
    !store
      .mutations
      .lock()
      .unwrap()
      .iter()
      .any(|mutation| mutation.kind == "tool_started")
  );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_summary_keeps_old_context_and_reopen_never_replays_unknown_io() {
  let server = MockModelServer::start(vec![
    support::text("one"),
    support::text("two"),
    support::Reply::stall(),
  ])
  .await
  .unwrap();
  let config = support::config(server.base_url());
  let store = support::shared();
  let runtime = Arc::new(
    Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
      .await
      .unwrap()
      .with_summary_guard(Arc::new(FixtureGuard)),
  );
  runtime.execute("task").await.unwrap();
  runtime.execute("followup").await.unwrap();
  let before = runtime.context_status().await.unwrap();
  let worker = runtime.clone();
  let compacting = tokio::spawn(async move { worker.compact("").await });
  server.wait_for_requests(3).await;
  runtime.cancel().await.unwrap();
  assert!(matches!(
    compacting.await.unwrap(),
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
  let after = runtime.context_status().await.unwrap();
  assert_eq!(after.message_count, before.message_count);
  assert!(!after.has_summary && after.summary_request_pending);
  drop(runtime);
  assert!(matches!(
    Runtime::open(config, store.clone(), vec![], Arc::new(|_| {})).await,
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
  assert_eq!(server.requests().await.len(), 3);
  let detached =
    agent_runtime::inspect_context(&support::config(server.base_url()), store.clone(), &[])
      .await
      .unwrap();
  assert!(detached.summary_request_pending && !detached.can_undo);
  assert_eq!(detached.message_count, before.message_count);
  let recovery = agent_runtime::inspect_recovery("session", 1, store)
    .await
    .unwrap();
  assert!(recovery.uncertainty.is_some());
}
