#[allow(dead_code)]
mod support;
use agent_core::events::{AgentEvent, QueueKind, QueueStatus};
use agent_runtime::{Result, Runtime, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct Gate {
  started: Arc<tokio::sync::Notify>,
  release: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl RuntimeTool for Gate {
  fn definition(&self) -> ToolDefinition {
    ToolDefinition {
      name: "inspect".into(),
      description: "Inspect fixture".into(),
      parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    }
  }
  async fn execute(&self, _: ToolCall, _: CancellationToken) -> Result<ToolOutput> {
    self.started.notify_one();
    self.release.notified().await;
    Ok(ToolOutput {
      text: "fixture contents".into(),
      is_error: false,
    })
  }
}
fn has_prompt(body: &Value, prompt: &str) -> bool {
  body["messages"]
    .as_array()
    .unwrap()
    .iter()
    .any(|message| message["role"] == "user" && message["content"].to_string().contains(prompt))
}
#[tokio::test]
async fn steer_and_follow_up_keep_the_framework_transcript_and_durable_receipts() {
  let mut server = support::Server::start(vec![
    support::tool(),
    support::text("steered answer"),
    support::text("follow-up answer"),
  ])
  .await;
  let store = support::shared();
  let started = Arc::new(tokio::sync::Notify::new());
  let release = Arc::new(tokio::sync::Notify::new());
  let runtime = Arc::new(
    Runtime::open(
      support::config(&server.endpoint),
      store.clone(),
      vec![Arc::new(Gate {
        started: started.clone(),
        release: release.clone(),
      })],
      Arc::new(|_| {}),
    )
    .await
    .unwrap(),
  );
  let execution = {
    let runtime = runtime.clone();
    tokio::spawn(async move { runtime.execute("initial prompt").await })
  };
  started.notified().await;
  let steer = runtime
    .enqueue_input("steer-request", QueueKind::Steer, "prioritize safety")
    .await
    .unwrap();
  let follow = runtime
    .enqueue_input("follow-request", QueueKind::FollowUp, "explain the result")
    .await
    .unwrap();
  assert_eq!(
    runtime
      .enqueue_input("steer-request", QueueKind::Steer, "prioritize safety")
      .await
      .unwrap(),
    steer
  );
  assert!(
    runtime
      .enqueue_input("steer-request", QueueKind::Steer, "different text")
      .await
      .is_err()
  );
  release.notify_one();
  let result = execution.await.unwrap().unwrap();
  assert_eq!(result.text, "follow-up answer");
  assert!(runtime.input_consumed(&steer).await.unwrap());
  assert!(runtime.input_consumed(&follow).await.unwrap());
  assert!(!runtime.has_pending_inputs().await.unwrap());
  server.request().await;
  let steered = server.request().await;
  let followed = server.request().await;
  assert!(has_prompt(&steered, "initial prompt"));
  assert!(has_prompt(&steered, "prioritize safety"));
  assert!(has_prompt(&followed, "prioritize safety"));
  assert!(has_prompt(&followed, "explain the result"));
  assert!(
    followed["messages"]
      .to_string()
      .contains("fixture contents")
  );
  let mutations = store.mutations.lock().unwrap();
  for id in [&steer, &follow] {
    let statuses: Vec<_> = mutations
      .iter()
      .flat_map(|mutation| &mutation.events)
      .filter_map(|event| match event {
        AgentEvent::QueueUpdated {
          entry_id, status, ..
        } if entry_id == id => Some(*status),
        _ => None,
      })
      .collect();
    assert_eq!(statuses, [QueueStatus::Pending, QueueStatus::Consumed]);
  }
  let usage: std::collections::BTreeSet<_> = mutations
    .iter()
    .flat_map(|mutation| &mutation.events)
    .filter_map(|event| match event {
      AgentEvent::UsageUpdated { entry_id, .. } => Some(entry_id),
      _ => None,
    })
    .collect();
  assert_eq!(usage.len(), 3, "input scheduling never aliases usage keys");
}
