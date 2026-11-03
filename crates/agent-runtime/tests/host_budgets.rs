#[allow(dead_code)]
mod support;
use agent_runtime::{Runtime, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use agent_test_support::MockModelServer;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;
struct OversizedTool(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl RuntimeTool for OversizedTool {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "Oversized synthetic output".into(),
      parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    }
  }
  async fn execute(&self, _: ToolCall, _: CancellationToken) -> agent_runtime::Result<ToolOutput> {
    self.0.fetch_add(1, Ordering::SeqCst);
    Ok(ToolOutput {
      text: "x".repeat(16 * 1024 * 1024 + 1),
      is_error: false,
    })
  }
}
#[tokio::test]
async fn oversized_confirmed_tool_result_persists_omission_receipt_without_reexecution() {
  let server = MockModelServer::start(vec![support::tool(), support::text("accepted receipt")])
    .await
    .unwrap();
  let store = support::shared();
  let config = support::config(server.base_url());
  let calls = Arc::new(AtomicUsize::new(0));
  let runtime = Runtime::open(
    config.clone(),
    store.clone(),
    vec![Arc::new(OversizedTool(calls.clone()))],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  assert_eq!(
    runtime.execute("inspect fixture").await.unwrap().outcome,
    agent_runtime::RunOutcome::Completed
  );
  let snapshot: serde_json::Value =
    serde_json::from_slice(store.snapshot.lock().unwrap().as_ref().unwrap()).unwrap();
  assert!(snapshot["pending_tools"].as_object().unwrap().is_empty());
  assert!(snapshot["active"].is_null());
  let requests = server.requests().await;
  assert_eq!(requests.len(), 2);
  assert!(
    requests[1]
      .body
      .to_string()
      .contains("result body was omitted")
  );
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  assert!(
    Runtime::open(config, store, vec![], Arc::new(|_| {}))
      .await
      .is_ok()
  );
}
#[tokio::test]
async fn oversized_provider_stream_keeps_last_confirmed_snapshot_and_request_intent() {
  let server = MockModelServer::start(vec![support::text(&"x".repeat(16 * 1024 * 1024 + 1))])
    .await
    .unwrap();
  let store = support::shared();
  let config = support::config(server.base_url());
  let runtime = Runtime::open(config.clone(), store.clone(), vec![], Arc::new(|_| {}))
    .await
    .unwrap();
  assert!(matches!(
    runtime.execute("bounded fixture").await,
    Err(RuntimeError::Uncertain(_))
  ));
  let snapshot: serde_json::Value =
    serde_json::from_slice(store.snapshot.lock().unwrap().as_ref().unwrap()).unwrap();
  assert_eq!(snapshot["active"]["in_flight"], "model");
  assert!(
    !store
      .mutations
      .lock()
      .unwrap()
      .iter()
      .any(|mutation| mutation.kind == "run_finished")
  );
  assert!(matches!(
    Runtime::open(config, store, vec![], Arc::new(|_| {})).await,
    Err(RuntimeError::Uncertain(_))
  ));
  assert_eq!(server.requests().await.len(), 1);
}

#[tokio::test]
async fn oversized_archive_is_rejected_without_overwriting_confirmed_storage_or_opening_io() {
  let store = support::shared();
  let pinned = rig_core::message::Message::user("x".repeat(64 * 1024 * 1024 + 1));
  let snapshot = serde_json::json!({
    "version":1,"session_id":"session",
    "context":{"pinned":pinned,"tasks":[],"log":[],"files":{},"revision":0,"pending":null},
    "active":null,"pending_tools":{},"queue":[],"uncertainty":null,
  });
  let bytes = serde_json::to_vec(&snapshot).unwrap();
  let expected_len = bytes.len();
  *store.snapshot.lock().unwrap() = Some(bytes);
  drop(snapshot);
  let result = Runtime::open(
    support::config("http://127.0.0.1:1"),
    store.clone(),
    vec![],
    Arc::new(|_| {}),
  )
  .await;
  assert!(matches!(
    result,
    Err(RuntimeError::Configuration(_)) | Err(RuntimeError::Persistence(_))
  ));
  assert_eq!(
    store.snapshot.lock().unwrap().as_ref().unwrap().len(),
    expected_len
  );
  assert!(store.mutations.lock().unwrap().is_empty());
}
