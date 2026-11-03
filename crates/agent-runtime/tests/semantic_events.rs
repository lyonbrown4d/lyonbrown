mod support;

use agent_core::events::AgentEvent;
use agent_runtime::{Runtime, RuntimeEvent, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

struct Inspect(Arc<support::Memory>);

#[async_trait::async_trait]
impl RuntimeTool for Inspect {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "Inspect fixture".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    }
  }

  async fn execute(
    &self,
    call: ToolCall,
    _: CancellationToken,
  ) -> agent_runtime::Result<ToolOutput> {
    assert_eq!(call.arguments, json!({"path":"file.txt"}));
    // A public tool-start fact must be committed before the host sees the call.
    assert!(
      self
        .0
        .mutations
        .lock()
        .expect("mutations")
        .iter()
        .flat_map(|mutation| &mutation.events)
        .any(|event| matches!(event, AgentEvent::ToolStarted{call_id,..} if call_id == &call.id))
    );
    Ok(ToolOutput {
      text: "fixture result".into(),
      is_error: false,
    })
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_adapter_persists_semantic_tool_and_usage_facts_without_duplicate_text() {
  let mut server = support::Server::start(vec![support::tool(), support::text("finished")]).await;
  let store = support::shared();
  let live = Arc::new(Mutex::new(Vec::new()));
  let capture = live.clone();
  let runtime = Runtime::open(
    support::config(&server.endpoint),
    store.clone(),
    vec![Arc::new(Inspect(store.clone()))],
    Arc::new(move |event| capture.lock().expect("events").push(event)),
  )
  .await
  .expect("runtime");
  let result = runtime.execute("inspect file").await.expect("execution");
  assert_eq!(result.text, "finished");
  assert_eq!(
    server.request().await["messages"][1]["content"],
    "inspect file"
  );
  assert!(
    server.request().await["messages"]
      .as_array()
      .expect("messages")
      .iter()
      .any(|message| message["role"] == "tool" && message["content"] == "fixture result")
  );
  let mutations = store.mutations.lock().expect("mutations");
  let semantic = mutations
    .iter()
    .flat_map(|mutation| &mutation.events)
    .collect::<Vec<_>>();
  assert_eq!(
    semantic
      .iter()
      .filter(|event| matches!(event, AgentEvent::ToolStarted { .. }))
      .count(),
    1
  );
  assert_eq!(semantic.iter().filter(|event|matches!(event, AgentEvent::ToolFinished{text,is_error:false,..} if text == "fixture result")).count(), 1);
  let usage = semantic
    .iter()
    .filter_map(|event| match event {
      AgentEvent::UsageUpdated { entry_id, usage } => Some((entry_id.as_str(), *usage)),
      _ => None,
    })
    .collect::<std::collections::BTreeMap<_, _>>();
  assert_eq!(
    usage.len(),
    2,
    "each assistant turn has its own durable usage key"
  );
  assert_eq!(usage.values().map(|usage| usage.total).sum::<u64>(), 23);
  assert!(
    mutations
      .iter()
      .filter(|mutation| mutation.kind == "model_response_committed")
      .any(|mutation| mutation
        .events
        .iter()
        .any(|event| matches!(event, AgentEvent::UsageUpdated { usage, .. } if usage.total == 14)))
  );
  assert!(!semantic.iter().any(|event| matches!(
    event,
    AgentEvent::TextDelta { .. } | AgentEvent::ThinkingDelta { .. }
  )));
  let live = live.lock().expect("events");
  assert_eq!(
    live
      .iter()
      .filter(|event| matches!(event, RuntimeEvent::ModelRequestStarted))
      .count(),
    2
  );
  assert_eq!(
    live
      .iter()
      .filter_map(|event| match event {
        RuntimeEvent::TextDelta { text } => Some(text.as_str()),
        _ => None,
      })
      .collect::<String>(),
    "finished"
  );
  let first_request = live
    .iter()
    .position(|event| matches!(event, RuntimeEvent::ModelRequestStarted))
    .expect("request boundary");
  let first_tool = live
    .iter()
    .position(|event| matches!(event, RuntimeEvent::ToolStarted { .. }))
    .expect("tool");
  assert!(first_request < first_tool);
}
