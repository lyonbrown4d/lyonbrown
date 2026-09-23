use std::sync::Arc;
use std::time::Duration;

use agent_core::{
  AgentError, RiskLevel, SecretRef, ToolDefinition, ToolInvocation, ToolResult, ToolSource,
};
use agent_kernel::{Kernel, KernelLimits, KernelState};
use agent_model::{ModelRegistry, Role};
use agent_secret::{NoopBroker, SecretBroker, SecretError};
use agent_tool::{
  AllowAllApproval, ApprovalDecision, ApprovalPolicy, EchoTool, Tool, ToolContext, ToolError,
  ToolRegistry,
};
use async_trait::async_trait;
use provider_mock::{MockProvider, ScriptedStep};
use serde_json::json;

fn kernel_with_script(script: Vec<ScriptedStep>, max_steps: usize) -> (Kernel, Arc<MockProvider>) {
  let mock = Arc::new(MockProvider::scripted(script));
  let kernel = kernel_with_mock(mock.clone(), max_steps, NoopBroker);
  (kernel, mock)
}

fn kernel_with_mock(
  mock: Arc<MockProvider>,
  max_steps: usize,
  broker: impl SecretBroker + 'static,
) -> Kernel {
  let tools = ToolRegistry::new();
  tools.register(Arc::new(EchoTool::new()));

  let providers = ModelRegistry::new();
  providers.register(mock);

  Kernel::builder()
    .providers(providers)
    .tools(tools)
    .approval(AllowAllApproval)
    .broker(broker)
    .limits(KernelLimits {
      max_steps_per_turn: max_steps,
      ..Default::default()
    })
    .build()
}

#[tokio::test]
async fn user_message_to_tool_call_to_answer() {
  let (mut kernel, _) = kernel_with_script(
    vec![
      ScriptedStep::tool_call("echo", json!({"text": "hello"})),
      ScriptedStep::text("The echo tool returned: hello."),
    ],
    20,
  );

  let events = kernel
    .run_turn("What does the echo tool say with 'hello'?")
    .await
    .unwrap();
  let kinds: Vec<_> = events.iter().map(|event| event.kind()).collect();

  assert_eq!(
    kinds,
    vec![
      "SessionStarted",
      "UserMessageReceived",
      "TurnStarted",
      "StepStarted",
      "ContextPrepared",
      "ModelSelected",
      "ModelStarted",
      "ModelDelta",
      "ModelCompleted",
      "ToolRequested",
      "ToolApproved",
      "ToolStarted",
      "ToolCompleted",
      "ObservationCreated",
      "StepCompleted",
      "StepStarted",
      "ContextPrepared",
      "ModelSelected",
      "ModelStarted",
      "ModelDelta",
      "ModelCompleted",
      "StepCompleted",
      "TurnCompleted",
    ]
  );

  let observation = events.iter().find_map(|event| event.tool_result()).unwrap();
  assert!(observation.ok);
  assert_eq!(observation.output, "hello");

  let second_request = kernel.last_requests().last().unwrap();
  assert!(
    second_request
      .messages
      .iter()
      .any(|message| message.role == Role::Tool && message.content == "hello")
  );

  let answer = events.iter().find_map(|event| event.turn_answer()).unwrap();
  assert!(answer.contains("hello"));
  assert_eq!(kernel.state(), &KernelState::Idle);
}

#[tokio::test]
async fn max_steps_stops_an_infinite_tool_loop() {
  let (mut kernel, _) = kernel_with_script(
    vec![
      ScriptedStep::tool_call("echo", json!({"text": "one"})),
      ScriptedStep::tool_call("echo", json!({"text": "two"})),
      ScriptedStep::tool_call("echo", json!({"text": "three"})),
    ],
    2,
  );

  let error = kernel.run_turn("loop").await.unwrap_err();

  assert!(matches!(error, AgentError::LimitExceeded(_)));
  assert_eq!(
    kernel
      .event_log()
      .iter()
      .filter(|event| event.kind() == "StepStarted")
      .count(),
    2
  );
  assert!(matches!(kernel.state(), KernelState::Failed { .. }));
}

#[tokio::test]
async fn cancellation_interrupts_a_pending_model_stream() {
  let (mut kernel, mock) = kernel_with_script(vec![ScriptedStep::pending()], 20);
  let cancellation = kernel.cancellation_token();

  let task = tokio::spawn(async move {
    let result = kernel.run_turn("wait forever").await;
    (kernel, result)
  });

  tokio::time::timeout(Duration::from_secs(1), async {
    while mock.request_count() == 0 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  cancellation.cancel();

  let (kernel, result) = task.await.unwrap();
  assert!(matches!(result, Err(AgentError::Cancelled)));
  assert_eq!(kernel.state(), &KernelState::Cancelled);
  assert_eq!(kernel.event_log().last().unwrap().kind(), "TurnCancelled");
}

#[tokio::test]
async fn provider_error_marks_the_kernel_failed_and_is_logged() {
  let (mut kernel, _) = kernel_with_script(vec![ScriptedStep::error("offline")], 20);

  let error = kernel.run_turn("fail").await.unwrap_err();

  assert!(matches!(error, AgentError::Model(_)));
  assert!(matches!(kernel.state(), KernelState::Failed { .. }));
  assert_eq!(kernel.event_log().last().unwrap().kind(), "Error");
}

#[tokio::test]
async fn cancellation_interrupts_pending_provider_discovery() {
  let mock = Arc::new(MockProvider::pending_discovery());
  let mut kernel = kernel_with_mock(mock.clone(), 20, NoopBroker);
  let cancellation = kernel.cancellation_token();
  let task = tokio::spawn(async move {
    let result = kernel.run_turn("wait during discovery").await;
    (kernel, result)
  });

  tokio::time::timeout(Duration::from_secs(1), async {
    while mock.discovery_count() == 0 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  cancellation.cancel();

  let (kernel, result) = tokio::time::timeout(Duration::from_secs(1), task)
    .await
    .unwrap()
    .unwrap();
  assert!(matches!(result, Err(AgentError::Cancelled)));
  assert_eq!(kernel.event_log().last().unwrap().kind(), "TurnCancelled");
}

#[path = "e2e_mock/extended.rs"]
mod extended;
