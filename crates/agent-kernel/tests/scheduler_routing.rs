use std::sync::{Arc, Mutex};

use agent_core::{AgentEvent, Locality, ModelCapabilities, ModelTier, StopReason, TokenUsage};
use agent_kernel::Kernel;
use agent_model::{
    ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest, ModelStream,
};
use agent_tool::{EchoTool, ToolRegistry};
use async_trait::async_trait;
use futures::stream;

struct RoutingProvider {
    requests: Mutex<Vec<ModelRequest>>,
}

impl RoutingProvider {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<ModelRequest> {
        self.requests
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
        Ok(vec![
            capabilities("control", ModelTier::Control, false),
            capabilities("worker", ModelTier::Worker, true),
        ])
    }

    async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
        let model = request.model.clone();
        self.requests
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
    let mut kernel = Kernel::builder().providers(registry).build();

    let events = kernel.run_turn("say hi").await.unwrap();

    let (selected_provider, model, reason) = selected(&events);
    assert_eq!(selected_provider, "routing-test");
    assert_eq!(model, "control");
    assert!(reason.contains("Low complexity"));
    assert_eq!(provider.requests()[0].model, "control");
}

#[tokio::test]
async fn step_with_tools_selects_worker_and_never_sends_tools_to_control() {
    let provider = Arc::new(RoutingProvider::new());
    let registry = ModelRegistry::new();
    registry.register(provider.clone());
    let tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool::new()));
    let mut kernel = Kernel::builder().providers(registry).tools(tools).build();

    let events = kernel.run_turn("say hi").await.unwrap();

    let (selected_provider, model, reason) = selected(&events);
    assert_eq!(selected_provider, "routing-test");
    assert_eq!(model, "worker");
    assert!(reason.contains("tools capability"));
    let requests = provider.requests();
    assert_eq!(requests[0].model, "worker");
    assert!(!requests[0].tools.is_empty());
    assert!(requests.iter().all(|request| request.model != "control"));
}
