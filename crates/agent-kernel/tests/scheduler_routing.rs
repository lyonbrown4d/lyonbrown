use std::sync::{Arc, Mutex};

use agent_core::{
  AgentEvent, ContextItem, ContextItemKind, DataClassification, Locality, ModelCapabilities,
  ModelTier, RiskLevel, StopReason, TokenUsage, ToolDefinition, ToolInvocation, ToolResult,
  ToolSource,
};
use agent_kernel::{Kernel, KernelModelOptions};
use agent_model::{
  ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest, ModelStream,
};
use agent_tool::{EchoTool, Tool, ToolContext, ToolError, ToolRegistry};
use async_trait::async_trait;
use futures::stream;

struct RoutingProvider {
  requests: Mutex<Vec<ModelRequest>>,
  context_window: u32,
}

impl RoutingProvider {
  fn new() -> Self {
    Self {
      requests: Mutex::new(Vec::new()),
      context_window: 8_192,
    }
  }

  fn with_context_window(context_window: u32) -> Self {
    Self {
      requests: Mutex::new(Vec::new()),
      context_window,
    }
  }

  fn requests(&self) -> Vec<ModelRequest> {
    self
      .requests
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .clone()
  }
}

#[async_trait]
impl ModelProvider for RoutingProvider {
  fn provider_id(&self) -> &'static str {
    "routing-test"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    let mut models = vec![
      capabilities("control", ModelTier::Control, false),
      capabilities("worker", ModelTier::Worker, true),
    ];
    for model in &mut models {
      model.context_window = self.context_window;
      model.max_output_tokens = model.max_output_tokens.min(self.context_window);
    }
    Ok(models)
  }

  async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
    let model = request.model.clone();
    self
      .requests
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .push(request);
    Ok(ModelStream {
      model,
      events: Box::pin(stream::iter([
        Ok(ModelEvent::Delta("ok".into())),
        Ok(ModelEvent::Usage(TokenUsage::default())),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::EndTurn,
        }),
      ])),
    })
  }
}

fn capabilities(model: &str, tier: ModelTier, supports_tools: bool) -> ModelCapabilities {
  ModelCapabilities {
    provider: "routing-test".into(),
    model: model.into(),
    tier,
    locality: Locality::Local,
    context_window: 8_192,
    max_output_tokens: 1_024,
    supports_tools,
    supports_streaming: true,
    supports_json: true,
    supports_thinking: false,
    cost_per_mtok_in: 0.0,
    cost_per_mtok_out: 0.0,
  }
}

fn selected(events: &[AgentEvent]) -> (&str, &str, &str) {
  events
    .iter()
    .find_map(|event| match event {
      AgentEvent::ModelSelected {
        provider,
        model,
        reason,
        ..
      } => Some((provider.as_str(), model.as_str(), reason.as_str())),
      _ => None,
    })
    .expect("a model should be selected")
}

#[tokio::test]
async fn simple_step_without_tools_selects_control_model() {
  let provider = Arc::new(RoutingProvider::new());
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let tools = ToolRegistry::new();
  tools.register(Arc::new(EchoTool::new()));
  let mut kernel = Kernel::builder().providers(registry).tools(tools).build();

  let events = kernel.run_turn("say hi").await.unwrap();

  let (selected_provider, model, reason) = selected(&events);
  assert_eq!(selected_provider, "routing-test");
  assert_eq!(model, "control");
  assert!(reason.contains("Low complexity"));
  assert_eq!(provider.requests()[0].model, "control");
  assert!(provider.requests()[0].tools.is_empty());
}

#[tokio::test]
async fn step_with_tools_selects_worker_and_never_sends_tools_to_control() {
  let provider = Arc::new(RoutingProvider::new());
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let tools = ToolRegistry::new();
  tools.register(Arc::new(EchoTool::new()));
  let mut kernel = Kernel::builder().providers(registry).tools(tools).build();

  let events = kernel
    .run_turn("list files in the workspace")
    .await
    .unwrap();

  let (selected_provider, model, reason) = selected(&events);
  assert_eq!(selected_provider, "routing-test");
  assert_eq!(model, "worker");
  assert!(reason.contains("tools capability"));
  let requests = provider.requests();
  assert_eq!(requests[0].model, "worker");
  assert!(!requests[0].tools.is_empty());
  assert!(requests.iter().all(|request| request.model != "control"));
}

#[tokio::test]
async fn compiled_context_reaches_provider_with_actual_event_counts() {
  use agent_context::TokenEstimator;
  use agent_profile::AgentProfile;
  use agent_skill::{Skill, SkillRegistry};
  use agent_workspace::WorkspaceProfile;

  let provider = Arc::new(RoutingProvider::new());
  let registry = ModelRegistry::new();
  registry.register(provider.clone());
  let workspace = WorkspaceProfile::detect(env!("CARGO_MANIFEST_DIR")).unwrap();
  let skills = SkillRegistry::new([Skill::from_yaml(
    "name: greeting\ntriggers: [hello]\ninstructions: Greet concisely.\n",
  )
  .unwrap()])
  .unwrap();
  let mut kernel = Kernel::builder()
    .providers(registry)
    .agent_profile(AgentProfile::builtin_default())
    .workspace_profile(workspace)
    .skills(skills)
    .build();

  let events = kernel.run_turn("hello").await.unwrap();
  let request = &provider.requests()[0];
  let system = request.system.as_deref().expect("compiled system prompt");
  assert!(system.contains("## Agent Profile"));
  assert!(system.contains("### greeting"));
  let prepared = events
    .iter()
    .find_map(|event| match event {
      AgentEvent::ContextPrepared {
        system_tokens,
        context_tokens,
        dropped,
        ..
      } => Some((*system_tokens, *context_tokens, *dropped)),
      _ => None,
    })
    .unwrap();
  assert!(prepared.0 > 0);
  assert!(prepared.1 > 0);
  assert_eq!(prepared.2, 0);
  let estimator = TokenEstimator::default();
  assert_eq!(
    u64::from(prepared.0),
    estimator.estimate(&serde_json::to_string(&request.system).unwrap())
  );
  assert_eq!(
    u64::from(prepared.1),
    estimator.estimate(
      &serde_json::to_string(&(request.messages.as_slice(), request.tools.as_slice())).unwrap()
    )
  );
  assert!(u64::from(prepared.0) + u64::from(prepared.1) <= u64::from(8_192 - request.max_tokens));
}

#[path = "scheduler_routing/extended.rs"]
mod extended;
