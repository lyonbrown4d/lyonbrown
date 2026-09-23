use std::sync::Arc;
use std::time::Duration;

use agent_core::{
  AgentError, AgentEvent, ContextItem, ContextItemKind, StepId, ToolCallId, ToolInvocation,
  ToolResult, TurnId,
};
use agent_kernel::Kernel;
use agent_model::{ModelRegistry, Role};
use agent_storage::SessionStore;
use agent_tool::{EchoTool, ToolRegistry};
use provider_mock::{MockProvider, ScriptedStep};
use serde_json::json;

fn memory_content(items: &[ContextItem]) -> Vec<(ContextItemKind, String, String)> {
  items
    .iter()
    .map(|item| (item.kind, item.content.clone(), item.source.clone()))
    .collect()
}

#[tokio::test]
async fn cancelled_turn_is_persisted_and_resume_replays_tool_context() {
  let path = std::env::temp_dir().join(format!(
    "lyonbrown-resume-{}-{}.db",
    std::process::id(),
    agent_core::SessionId::new()
  ));
  let store = SessionStore::open(&path).await.unwrap();
  let first_provider = Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("echo", json!({"text": "from first step"})),
    ScriptedStep::pending(),
  ]));
  let first_registry = ModelRegistry::new();
  first_registry.register(first_provider.clone());
  let tools = ToolRegistry::new();
  tools.register(Arc::new(EchoTool::new()));
  let mut first = Kernel::builder()
    .providers(first_registry)
    .tools(tools)
    .storage(store.clone())
    .build();
  let session_id = first.session_id();
  let cancellation = first.cancellation_token();
  let task = tokio::spawn(async move {
    let result = first.run_turn("run the echo tool").await;
    (first, result)
  });
  tokio::time::timeout(Duration::from_secs(2), async {
    while first_provider.request_count() < 2 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  cancellation.cancel();
  let (mut first, result) = task.await.unwrap();
  assert!(matches!(result, Err(AgentError::Cancelled)));
  let before_resume = memory_content(&first.session().working_memory);
  let saved = store.events(session_id).await.unwrap();
  assert_eq!(saved, first.event_log());
  assert!(saved.iter().any(|event| event.kind() == "TurnCancelled"));

  let second_provider = Arc::new(MockProvider::scripted(vec![ScriptedStep::text("resumed")]));
  let second_registry = ModelRegistry::new();
  second_registry.register(second_provider.clone());
  let mut resumed = Kernel::builder()
    .providers(second_registry)
    .storage(store.clone())
    .build();
  resumed.resume(&saved).unwrap();
  assert_eq!(resumed.session_id(), session_id);
  assert!(resumed.session().started);
  assert_eq!(
    memory_content(&resumed.session().working_memory),
    before_resume
  );
  resumed.run_turn("continue").await.unwrap();
  let request = &second_provider.requests()[0];
  assert!(
    request
      .messages
      .iter()
      .any(|message| { message.role == Role::Tool && message.content == "from first step" })
  );
  assert!(
    request
      .messages
      .iter()
      .any(|message| { message.role == Role::User && message.content == "run the echo tool" })
  );
  assert!(store.events(session_id).await.unwrap().len() > saved.len());
  first.close_storage().await.unwrap();
  resumed.close_storage().await.unwrap();
  drop(first);
  drop(resumed);
  drop(store);
  std::fs::remove_file(&path).unwrap();
}

#[tokio::test]
async fn resume_keeps_completed_and_uncertain_calls_from_interrupted_batch() {
  let session_id = agent_core::SessionId::new();
  let turn_id = TurnId::new();
  let step_id = StepId::new();
  let completed = ToolInvocation {
    id: ToolCallId::new(),
    name: "echo".into(),
    arguments: json!({"text": "done"}),
  };
  let uncertain = ToolInvocation {
    id: ToolCallId::new(),
    name: "write_file".into(),
    arguments: json!({"path": "file.txt"}),
  };
  let now = chrono::Utc::now();
  let events = vec![
    AgentEvent::SessionStarted {
      session_id,
      ts: now,
    },
    AgentEvent::UserMessageReceived {
      session_id,
      turn_id,
      text: "do two things".into(),
      ts: now,
    },
    AgentEvent::TurnStarted {
      session_id,
      turn_id,
      ts: now,
    },
    AgentEvent::StepStarted {
      session_id,
      turn_id,
      step_id,
      ts: now,
    },
    AgentEvent::ToolRequested {
      step_id,
      invocation: completed.clone(),
      ts: now,
    },
    AgentEvent::ToolStarted {
      step_id,
      tool_call_id: completed.id,
      ts: now,
    },
    AgentEvent::ToolCompleted {
      step_id,
      result: ToolResult {
        invocation_id: completed.id,
        ok: true,
        output: "done".into(),
        truncated: false,
        duration_ms: 0,
        error: None,
      },
      ts: now,
    },
    AgentEvent::ToolRequested {
      step_id,
      invocation: uncertain.clone(),
      ts: now,
    },
    AgentEvent::ToolStarted {
      step_id,
      tool_call_id: uncertain.id,
      ts: now,
    },
    AgentEvent::TurnCancelled {
      session_id,
      turn_id,
      ts: now,
    },
  ];
  let provider = Arc::new(MockProvider::scripted(vec![ScriptedStep::text(
    "continued",
  )]));
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let mut kernel = Kernel::builder().providers(registry).build();
  kernel.resume(&events).unwrap();
  kernel.run_turn("continue safely").await.unwrap();
  let request = &provider.requests()[0];
  assert!(request.messages.iter().any(|message| {
    message.role == Role::Tool
      && message.tool_call_id == Some(completed.id)
      && message.content == "done"
  }));
  assert!(request.messages.iter().any(|message| {
    message.role == Role::Tool
      && message.tool_call_id == Some(uncertain.id)
      && message.content.contains("outcome unknown")
  }));
  assert!(request.messages.iter().any(|message| {
    message.role == Role::Assistant
      && message
        .tool_calls
        .iter()
        .any(|call| call.id == completed.id)
      && message
        .tool_calls
        .iter()
        .any(|call| call.id == uncertain.id)
  }));

  let history = kernel.event_log().to_vec();
  let next_provider = Arc::new(MockProvider::scripted(vec![ScriptedStep::text("again")]));
  let next_registry = ModelRegistry::new();
  next_registry.register(next_provider.clone());
  let mut next = Kernel::builder().providers(next_registry).build();
  next.resume(&history).unwrap();
  next.run_turn("one more").await.unwrap();
  let messages = &next_provider.requests()[0].messages;
  let uncertain_index = messages
    .iter()
    .position(|message| message.tool_call_id == Some(uncertain.id))
    .unwrap();
  let continued_index = messages
    .iter()
    .position(|message| message.role == Role::User && message.content == "continue safely")
    .unwrap();
  assert!(uncertain_index < continued_index);
}
