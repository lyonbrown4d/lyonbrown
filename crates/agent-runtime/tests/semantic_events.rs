#[path = "../src/semantic_events.rs"]
mod semantic_adapter;
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
  assert!(mutations.iter().filter(|mutation| mutation.kind == "entry_appended").any(|mutation| mutation.events.iter().any(|event| matches!(event, AgentEvent::UsageUpdated { entry_id, usage } if mutation.payload["entry"]["id"] == *entry_id && usage.total == 14))));
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

#[test]
fn compaction_and_later_usage_record_share_the_same_replacement_key() {
  use rpi_harness::session::types::{
    CompactionEntry, Entry, EntryBase, LaneRecord, RecordBase, UsageCause, UsageRecord,
  };
  let mut reported = rpi_ai::Usage::zero();
  reported.input = 7;
  reported.output = 2;
  reported.total_tokens = 9;
  let mut compacted = CompactionEntry {
    base: EntryBase {
      entry_type: "compaction".into(),
      id: "compacted".into(),
      seq: 1,
      parent_id: None,
      timestamp: 1,
    },
    summary: "fixture summary".into(),
    retained_tail: Vec::new(),
    tokens_before: 100,
    details: None,
    usage: None,
  };
  assert!(
    semantic_adapter::entry(&Entry::Compaction(compacted.clone()), &[]).is_empty(),
    "missing usage must not be estimated"
  );
  compacted.usage = Some(reported.clone());
  let mut events = semantic_adapter::entry(&Entry::Compaction(compacted), &[]);
  reported.output = 3;
  reported.total_tokens = 10;
  events.extend(semantic_adapter::record(
    &LaneRecord::Usage(UsageRecord {
      base: RecordBase {
        id: "correction".into(),
        seq: 2,
        lane: "main".into(),
        timestamp: 2,
      },
      usage: reported,
      cause: UsageCause::Compaction,
      run_id: Some("run".into()),
      entry_id: Some("compacted".into()),
      attempt: Some(1),
      stop_reason: None,
      tool_call_id: None,
      details: None,
    }),
    &[],
  ));
  let usage = events
    .into_iter()
    .filter_map(|event| match event {
      AgentEvent::UsageUpdated { entry_id, usage } => Some((entry_id, usage)),
      _ => None,
    })
    .collect::<std::collections::BTreeMap<_, _>>();
  assert_eq!(usage.len(), 1);
  assert_eq!(usage["compacted"].input, 7);
  assert_eq!(usage["compacted"].output, 3);
  assert_eq!(usage["compacted"].total, 10);
}

#[test]
fn tool_usage_is_reported_only_when_present_on_the_committed_result() {
  use rpi_harness::session::types::{Entry, EntryBase, MessageEntry};
  let mut tool = rpi_ai::ToolResultMessage {
    role: Default::default(),
    tool_call_id: "call".into(),
    tool_name: "fixture".into(),
    content: Vec::new(),
    details: None,
    usage: None,
    added_tool_names: Vec::new(),
    is_error: false,
    timestamp: 1,
  };
  let entry = |tool| {
    Entry::Message(MessageEntry {
      base: EntryBase {
        entry_type: "message".into(),
        id: "tool-result".into(),
        seq: 1,
        parent_id: None,
        timestamp: 1,
      },
      message: rpi_agent::message::AgentMessage::ToolResult(Box::new(tool)),
      terminate: None,
    })
  };
  assert!(
    semantic_adapter::entry(&entry(tool.clone()), &[])
      .iter()
      .all(|event| !matches!(event, AgentEvent::UsageUpdated { .. }))
  );
  let mut usage = rpi_ai::Usage::zero();
  usage.input = 5;
  usage.total_tokens = 5;
  tool.usage = Some(usage);
  let events = semantic_adapter::entry(&entry(tool), &[]);
  assert!(
    events
      .iter()
      .any(|event| matches!(event, AgentEvent::ToolFinished { call_id, .. } if call_id == "call"))
  );
  assert!(events.iter().any(|event|matches!(event, AgentEvent::UsageUpdated { entry_id, usage } if entry_id == "tool-result" && usage.total == 5)));
}
