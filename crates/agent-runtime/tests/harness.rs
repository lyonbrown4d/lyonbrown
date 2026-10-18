mod support;
use agent_runtime::{
  RunOutcome, Runtime, RuntimeEvent, RuntimeTool, ToolCall, ToolDefinition, ToolOutput,
};
use serde_json::json;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use support::{Reply, Server, config, shared, text, tool};
use tokio_util::sync::CancellationToken;

struct Inspect {
  store: Arc<support::Memory>,
  calls: Arc<AtomicUsize>,
  started: Option<Arc<tokio::sync::Notify>>,
}
#[async_trait::async_trait]
impl RuntimeTool for Inspect {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "Inspect a file".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
    }
  }
  async fn execute(
    &self,
    call: ToolCall,
    _: CancellationToken,
  ) -> agent_runtime::Result<ToolOutput> {
    let snapshot = self.store.snapshot.lock().unwrap().clone().unwrap();
    let durable: serde_json::Value = serde_json::from_slice(&snapshot).unwrap();
    assert!(
      durable["state"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["type"] == "tool_started")
    );
    assert_eq!(call.arguments, json!({"path":"file.txt"}));
    self.calls.fetch_add(1, Ordering::SeqCst);
    if let Some(started) = &self.started {
      started.notify_one();
      std::future::pending::<()>().await;
    }
    Ok(ToolOutput {
      text: "file contents".into(),
      is_error: false,
    })
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_harness_drives_tools_streams_and_restores_transcript() {
  let mut server = Server::start(vec![tool(), text("inspected"), text("remembered")]).await;
  let store = shared();
  let calls = Arc::new(AtomicUsize::new(0));
  let events = Arc::new(std::sync::Mutex::new(Vec::new()));
  let capture = events.clone();
  let runtime = Runtime::open(
    config(&server.endpoint),
    store.clone(),
    vec![Arc::new(Inspect {
      store: store.clone(),
      calls: calls.clone(),
      started: None,
    })],
    Arc::new(move |event| capture.lock().unwrap().push(event)),
  )
  .await
  .unwrap();
  let result = runtime.execute("inspect file").await.unwrap();
  assert_eq!(result.outcome, RunOutcome::Completed);
  assert_eq!(result.text, "inspected");
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  assert!(
    events
      .lock()
      .unwrap()
      .iter()
      .any(|event| matches!(event,RuntimeEvent::TextDelta { text } if text == "inspected"))
  );
  assert!(
    !String::from_utf8(store.snapshot.lock().unwrap().clone().unwrap())
      .unwrap()
      .contains("fixture-secret-never-persist")
  );
  drop(runtime);
  let runtime = Runtime::open(
    config(&server.endpoint),
    store,
    Vec::new(),
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  assert!(runtime.resume().await.unwrap().is_none());
  assert_eq!(
    runtime.execute("what did you inspect?").await.unwrap().text,
    "remembered"
  );
  server.request().await;
  let tool_context = server.request().await;
  assert!(
    tool_context["messages"]
      .as_array()
      .unwrap()
      .iter()
      .any(|message| message["role"] == "tool" && message["content"] == "file contents")
  );
  let restored = server.request().await;
  assert!(
    restored["messages"]
      .as_array()
      .unwrap()
      .iter()
      .any(|message| message["content"] == "inspect file")
  );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_reaches_stalled_provider() {
  let mut server = Server::start(vec![Reply::stall()]).await;
  let runtime = Arc::new(
    Runtime::open(
      config(&server.endpoint),
      shared(),
      Vec::new(),
      Arc::new(|_| {}),
    )
    .await
    .unwrap(),
  );
  let execution = {
    let runtime = runtime.clone();
    tokio::spawn(async move { runtime.execute("wait").await })
  };
  tokio::time::timeout(std::time::Duration::from_secs(5), server.request())
    .await
    .unwrap();
  runtime.cancel().await.unwrap();
  let result = tokio::time::timeout(std::time::Duration::from_secs(5), execution)
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  assert_eq!(result.outcome, RunOutcome::Cancelled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_tool_is_uncertain_and_never_replayed() {
  let _server = Server::start(vec![tool()]).await;
  let store = shared();
  let calls = Arc::new(AtomicUsize::new(0));
  let started = Arc::new(tokio::sync::Notify::new());
  let runtime = Arc::new(
    Runtime::open(
      config(&_server.endpoint),
      store.clone(),
      vec![Arc::new(Inspect {
        store: store.clone(),
        calls: calls.clone(),
        started: Some(started.clone()),
      })],
      Arc::new(|_| {}),
    )
    .await
    .unwrap(),
  );
  let execution = {
    let runtime = runtime.clone();
    tokio::spawn(async move { runtime.execute("inspect").await })
  };
  tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
    .await
    .unwrap();
  let snapshot = store.snapshot.lock().unwrap().clone();
  execution.abort();
  let _ = execution.await;
  let restarted = shared();
  *restarted.snapshot.lock().unwrap() = snapshot;
  let reopened = Runtime::open(
    config(&_server.endpoint),
    restarted,
    Vec::new(),
    Arc::new(|_| {}),
  )
  .await;
  assert!(matches!(
    reopened,
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
  assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct UnknownOutcome;
#[async_trait::async_trait]
impl RuntimeTool for UnknownOutcome {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "Inspect".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    }
  }
  async fn execute(&self, _: ToolCall, _: CancellationToken) -> agent_runtime::Result<ToolOutput> {
    Err(agent_runtime::RuntimeError::Uncertain(
      "remote effect could not be confirmed".into(),
    ))
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_reported_uncertainty_stops_run_and_survives_restart() {
  let server = Server::start(vec![tool()]).await;
  let store = shared();
  let runtime = Runtime::open(
    config(&server.endpoint),
    store.clone(),
    vec![Arc::new(UnknownOutcome)],
    Arc::new(|_| {}),
  )
  .await
  .unwrap();
  let result = tokio::time::timeout(
    std::time::Duration::from_secs(5),
    runtime.execute("inspect"),
  )
  .await
  .unwrap();
  assert!(matches!(
    result,
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
  drop(runtime);
  assert!(matches!(
    Runtime::open(
      config(&server.endpoint),
      store,
      Vec::new(),
      Arc::new(|_| {})
    )
    .await,
    Err(agent_runtime::RuntimeError::Uncertain(_))
  ));
}

#[tokio::test]
async fn unsupported_tokio_flavor_is_reported_before_harness_callbacks() {
  let result = Runtime::open(
    config("https://unused.invalid"),
    shared(),
    Vec::new(),
    Arc::new(|_| {}),
  )
  .await;
  assert!(
    matches!(result,Err(agent_runtime::RuntimeError::Configuration(message)) if message.contains("multithreaded Tokio"))
  );
}
