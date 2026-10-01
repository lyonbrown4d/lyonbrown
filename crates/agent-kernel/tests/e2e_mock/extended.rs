use super::*;

#[tokio::test]
async fn cancellation_interrupts_pending_stream_creation() {
  let (mut kernel, mock) = kernel_with_script(vec![ScriptedStep::pending_start()], 20);
  let cancellation = kernel.cancellation_token();
  let task = tokio::spawn(async move {
    let result = kernel.run_turn("wait during stream creation").await;
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

  let (kernel, result) = tokio::time::timeout(Duration::from_secs(1), task)
    .await
    .unwrap()
    .unwrap();
  assert!(matches!(result, Err(AgentError::Cancelled)));
  assert_eq!(kernel.event_log().last().unwrap().kind(), "TurnCancelled");
}

#[tokio::test]
async fn unknown_tool_is_returned_to_the_model_as_a_failed_observation() {
  let (mut kernel, _) = kernel_with_script(
    vec![
      ScriptedStep::tool_call("missing", json!({})),
      ScriptedStep::text("I recovered from the tool error."),
    ],
    20,
  );

  let events = kernel.run_turn("use a missing tool").await.unwrap();

  let result = events.iter().find_map(|event| event.tool_result()).unwrap();
  assert!(!result.ok);
  assert!(result.error.as_deref().unwrap().contains("tool not found"));
  assert!(
    kernel.last_requests()[1]
      .messages
      .iter()
      .any(|message| message.role == Role::Tool && message.content.contains("tool not found"))
  );
}

#[tokio::test]
async fn denied_tool_is_returned_as_an_observation_and_the_turn_continues() {
  let mock = Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("echo", json!({"text": "must not execute"})),
    ScriptedStep::text("I continued after the denial."),
  ]));
  let providers = ModelRegistry::new();
  providers.register(mock);
  let tools = ToolRegistry::new();
  tools.register(Arc::new(EchoTool::new()));
  let mut kernel = Kernel::builder()
    .providers(providers)
    .tools(tools)
    .approval_arc(Arc::new(DenyApproval))
    .build();

  let events = kernel.run_turn("try the denied tool").await.unwrap();

  let result = events.iter().find_map(|event| event.tool_result()).unwrap();
  assert!(!result.ok);
  assert_eq!(
    result.error.as_deref(),
    Some("tool denied by approval policy")
  );
  assert!(!events.iter().any(|event| event.kind() == "ToolStarted"));
  assert_eq!(events.last().unwrap().kind(), "TurnCompleted");
  assert_eq!(
    events.iter().find_map(|event| event.turn_answer()),
    Some("I continued after the denial.")
  );
}

#[tokio::test]
async fn kernel_awaits_approval_with_the_typed_tool_preview() {
  let mock = Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("echo", json!({"text": "approved"})),
    ScriptedStep::text("done"),
  ]));
  let providers = ModelRegistry::new();
  providers.register(mock);
  let tools = ToolRegistry::new();
  tools.register(Arc::new(EchoTool::new()));
  let captured = Arc::new(std::sync::Mutex::new(None));
  let mut kernel = Kernel::builder()
    .providers(providers)
    .tools(tools)
    .approval(CapturingApproval(captured.clone()))
    .build();

  let events = kernel.run_turn("approve the tool").await.unwrap();

  let preview = captured.lock().unwrap().clone().unwrap();
  assert_eq!(preview.tool, "echo");
  assert_eq!(preview.risk, RiskLevel::Read);
  assert!(events.iter().any(|event| event.kind() == "ToolApproved"));
}

#[tokio::test]
async fn tool_output_is_redacted_before_events_and_model_observations() {
  let mock = Arc::new(MockProvider::scripted(vec![
    ScriptedStep::tool_call("fixed_secret", json!({})),
    ScriptedStep::text("done"),
  ]));
  let providers = ModelRegistry::new();
  providers.register(mock);
  let tools = ToolRegistry::new();
  tools.register(Arc::new(FixedSecretTool::new()));
  let mut kernel = Kernel::builder()
    .providers(providers)
    .tools(tools)
    .approval(AllowAllApproval)
    .broker(RedactingBroker)
    .build();

  let events = kernel.run_turn("echo a secret").await.unwrap();

  let serialized = serde_json::to_string(&events).unwrap();
  assert!(!serialized.contains("secret-value"));
  assert!(
    kernel.last_requests()[1]
      .messages
      .iter()
      .any(|message| { message.role == Role::Tool && message.content == "[REDACTED]" })
  );
}

#[tokio::test]
async fn tool_use_without_calls_is_logged_as_a_provider_protocol_error() {
  let (mut kernel, _) = kernel_with_script(vec![ScriptedStep::empty_tool_use()], 20);

  let error = kernel.run_turn("invalid tool response").await.unwrap_err();

  assert!(matches!(error, AgentError::Model(_)));
  assert!(matches!(kernel.state(), KernelState::Failed { .. }));
  assert_eq!(kernel.event_log().last().unwrap().kind(), "Error");
}

#[tokio::test]
async fn oversized_tool_arguments_cannot_bypass_next_step_budget() {
  let arguments = "x".repeat(256 * 1024);
  let (mut kernel, _) = kernel_with_script(
    vec![
      ScriptedStep::tool_call("echo", json!({"text": arguments})),
      ScriptedStep::text("must not be requested"),
    ],
    20,
  );

  let error = kernel.run_turn("run a shell command").await.unwrap_err();

  assert!(matches!(error, AgentError::Context(_)));
  assert_eq!(kernel.last_requests().len(), 1);
  assert_eq!(kernel.event_log().last().unwrap().kind(), "Error");
}

struct RedactingBroker;

struct DenyApproval;

#[async_trait]
impl ApprovalPolicy for DenyApproval {
  async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse {
    ApprovalResponse {
      request_id: request.request_id,
      decision: ApprovalDecision::Deny,
    }
  }
}

struct CapturingApproval(Arc<std::sync::Mutex<Option<agent_core::ToolPreview>>>);

#[async_trait]
impl ApprovalPolicy for CapturingApproval {
  async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse {
    tokio::task::yield_now().await;
    *self.0.lock().unwrap() = Some(request.preview);
    ApprovalResponse {
      request_id: request.request_id,
      decision: ApprovalDecision::Allow,
    }
  }
}

struct FixedSecretTool {
  definition: ToolDefinition,
}

impl FixedSecretTool {
  fn new() -> Self {
    Self {
      definition: ToolDefinition {
        name: "fixed_secret".into(),
        description: "returns a fixed test secret".into(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Read,
        requires_secrets: vec![],
      },
    }
  }
}

#[async_trait]
impl Tool for FixedSecretTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    _context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    Ok(ToolResult {
      invocation_id: invocation.id,
      ok: true,
      output: "secret-value".into(),
      truncated: false,
      duration_ms: 0,
      error: None,
    })
  }
}

#[async_trait]
impl SecretBroker for RedactingBroker {
  async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError> {
    Err(SecretError::NotFound(reference.clone()))
  }

  fn redact(&self, text: &str) -> String {
    text.replace("secret-value", "[REDACTED]")
  }
}
