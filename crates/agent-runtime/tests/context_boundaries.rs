#[allow(dead_code)]
mod support;
use agent_runtime::{
  Runtime, RuntimeError, RuntimeTool, SessionMutation, SessionPersistence, SummaryInput,
  SummaryRequestGuard, ToolCall, ToolDefinition, ToolOutput,
};
use agent_test_support::MockModelServer;
use serde_json::json;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
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

struct Inspect(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl RuntimeTool for Inspect {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "Synthetic inspection".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    }
  }
  async fn execute(&self, _: ToolCall, _: CancellationToken) -> agent_runtime::Result<ToolOutput> {
    self.0.fetch_add(1, Ordering::SeqCst);
    Ok(ToolOutput {
      text: "recent-result ".repeat(1700),
      is_error: false,
    })
  }
}

#[tokio::test]
async fn automatic_compaction_keeps_recent_tool_call_and_result_together() {
  let server = MockModelServer::start(vec![
    support::text(&"older verified output ".repeat(1600)),
    support::tool(),
    support::text("checkpoint of older work"),
    support::text("tool result accepted"),
  ])
  .await
  .unwrap();
  let mut config = support::config(server.base_url());
  config.context_window = 16_000;
  config.max_output_tokens = 1000;
  let calls = Arc::new(AtomicUsize::new(0));
  let store = support::shared();
  let runtime = Runtime::open(
    config,
    store.clone(),
    vec![Arc::new(Inspect(calls.clone()))],
    Arc::new(|_| {}),
  )
  .await
  .unwrap()
  .with_summary_guard(Arc::new(Guard));
  runtime
    .execute("preserve the exact original task")
    .await
    .unwrap();
  let result = runtime.execute("inspect file").await.unwrap();
  assert_eq!(result.text, "tool result accepted");
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  let requests = server.requests().await;
  assert_eq!(requests.len(), 4);
  let messages = requests[3].body["messages"].as_array().unwrap();
  assert!(messages.iter().any(|message| {
    message["tool_calls"]
      .as_array()
      .is_some_and(|calls| calls.iter().any(|call| call["id"] == "call_fixture"))
  }));
  assert!(
    messages
      .iter()
      .any(|message| message["role"] == "tool" && message["tool_call_id"] == "call_fixture")
  );
  assert!(
    messages
      .iter()
      .any(|message| message.to_string().contains("checkpoint of older work"))
  );
  assert!(runtime.context_status().await.unwrap().has_summary);
  let snapshot: serde_json::Value =
    serde_json::from_slice(store.snapshot.lock().unwrap().as_ref().unwrap()).unwrap();
  assert!(snapshot["pending_tools"].as_object().unwrap().is_empty());
}

struct FencedStore(Arc<support::Memory>);
#[async_trait::async_trait]
impl SessionPersistence for FencedStore {
  async fn load(&self, id: &str) -> agent_runtime::Result<Option<Vec<u8>>> {
    self.0.load(id).await
  }
  async fn commit(
    &self,
    id: &str,
    epoch: u64,
    snapshot: Vec<u8>,
    mutation: SessionMutation,
  ) -> agent_runtime::Result<()> {
    if mutation.kind == "context_compaction_committed" {
      return Err(RuntimeError::Persistence(
        "fixture owner epoch changed".into(),
      ));
    }
    self.0.commit(id, epoch, snapshot, mutation).await
  }
}

#[tokio::test]
async fn refused_owner_fence_never_publishes_summary_and_fences_further_execution() {
  let server = MockModelServer::start(vec![
    support::text("old"),
    support::text("recent"),
    support::text("summary"),
  ])
  .await
  .unwrap();
  let memory = support::shared();
  let store = Arc::new(FencedStore(memory.clone()));
  let config = support::config(server.base_url());
  let runtime = Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap()
    .with_summary_guard(Arc::new(Guard));
  runtime.execute("task").await.unwrap();
  runtime.execute("followup").await.unwrap();
  let before = runtime.context_status().await.unwrap();
  assert!(matches!(
    runtime.compact("").await,
    Err(RuntimeError::Uncertain(_))
  ));
  let after = runtime.context_status().await.unwrap();
  assert_eq!(after.message_count, before.message_count);
  assert!(!after.has_summary && after.summary_request_pending);
  assert!(matches!(
    runtime.execute("cannot continue").await,
    Err(RuntimeError::Uncertain(_))
  ));
  assert!(matches!(
    Runtime::open(config, store, vec![], Arc::new(|_| {})).await,
    Err(RuntimeError::Uncertain(_))
  ));
  assert!(
    !memory
      .mutations
      .lock()
      .unwrap()
      .iter()
      .any(|mutation| mutation.kind == "context_compaction_committed")
  );
  assert_eq!(server.requests().await.len(), 3);
}
