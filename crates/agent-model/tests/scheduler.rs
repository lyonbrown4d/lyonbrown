use std::sync::Arc;

use agent_core::{DataClassification, Locality, ModelCapabilities, ModelTier};
use agent_model::{
    Complexity, ModelError, ModelProvider, ModelRegistry, ModelRequest, ModelScheduler,
    ModelStream, RoutePhase, RoutingPolicy, StepContext,
};
use async_trait::async_trait;

struct StaticProvider {
    id: &'static str,
    models: Vec<ModelCapabilities>,
}

#[async_trait]
impl ModelProvider for StaticProvider {
    fn provider_id(&self) -> &'static str {
        self.id
    }

    async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
        Ok(self.models.clone())
    }

    async fn stream(&self, _req: ModelRequest) -> Result<ModelStream, ModelError> {
        unreachable!("scheduler tests never run inference")
    }
}

fn model(
    provider: &str,
    name: &str,
    tier: ModelTier,
    locality: Locality,
    tools: bool,
) -> ModelCapabilities {
    ModelCapabilities {
        provider: provider.into(),
        model: name.into(),
        tier,
        locality,
        context_window: 32_768,
        max_output_tokens: 4_096,
        supports_tools: tools,
        supports_streaming: true,
        supports_json: true,
        supports_thinking: tier != ModelTier::Control,
        cost_per_mtok_in: if locality == Locality::Local {
            0.0
        } else {
            3.0
        },
        cost_per_mtok_out: if locality == Locality::Local {
            0.0
        } else {
            15.0
        },
    }
}

fn scheduler() -> ModelScheduler {
    let registry = ModelRegistry::new();
    registry.register(Arc::new(StaticProvider {
        id: "ollama",
        models: vec![
            model(
                "ollama",
                "translategemma:4b",
                ModelTier::Control,
                Locality::Local,
                false,
            ),
            model(
                "ollama",
                "qwen3.8-27b:q3-claude",
                ModelTier::Worker,
                Locality::Local,
                true,
            ),
        ],
    }));
    registry.register(Arc::new(StaticProvider {
        id: "remote",
        models: vec![model(
            "remote",
            "frontier",
            ModelTier::Frontier,
            Locality::Remote,
            true,
        )],
    }));
    ModelScheduler::new(registry, RoutingPolicy::default())
}

#[tokio::test]
async fn simple_input_selects_control_model() {
    let selection = scheduler()
        .decide(&StepContext::new("say hi"))
        .await
        .unwrap();

    assert_eq!(selection.capabilities.tier, ModelTier::Control);
    assert_eq!(selection.decision.model, "translategemma:4b");
    assert_eq!(selection.decision.phase, RoutePhase::Pre);
    assert_eq!(selection.complexity, Complexity::Low);
}

#[tokio::test]
async fn code_input_selects_worker_model() {
    let selection = scheduler()
        .decide(&StepContext::new(
            "Fix this Rust function:\n```rust\nfn main() {}\n```",
        ))
        .await
        .unwrap();

    assert_eq!(selection.capabilities.tier, ModelTier::Worker);
    assert_eq!(selection.decision.model, "qwen3.8-27b:q3-claude");
    assert_eq!(selection.complexity, Complexity::High);
}

#[tokio::test]
async fn tool_step_never_selects_model_without_tool_capability() {
    let mut context = StepContext::new("what is the weather?");
    context.needs_tools = true;

    let selection = scheduler().decide(&context).await.unwrap();

    assert!(selection.capabilities.supports_tools);
    assert_ne!(selection.capabilities.tier, ModelTier::Control);
    assert!(selection.decision.reason.contains("tools"));
}

#[tokio::test]
async fn confidential_input_with_local_preference_filters_remote_models() {
    let mut context = StepContext::new("review this confidential note");
    context.classification = DataClassification::Confidential;
    context.escalated = true;

    let selection = scheduler().decide(&context).await.unwrap();

    assert_eq!(selection.capabilities.locality, Locality::Local);
}

#[tokio::test]
async fn explicit_incapable_model_is_rejected_for_tool_step() {
    let mut context = StepContext::new("use a tool");
    context.needs_tools = true;
    context.provider = Some("ollama".into());
    context.model = Some("translategemma:4b".into());

    let error = match scheduler().decide(&context).await {
        Ok(_) => panic!("incapable explicit model should be rejected"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("tool"));
}

#[tokio::test]
async fn remote_model_over_cost_budget_is_filtered() {
    let registry = ModelRegistry::new();
    registry.register(Arc::new(StaticProvider {
        id: "remote",
        models: vec![model(
            "remote",
            "frontier",
            ModelTier::Frontier,
            Locality::Remote,
            true,
        )],
    }));
    let scheduler = ModelScheduler::new(
        registry,
        RoutingPolicy {
            prefer_local: false,
            remote_escalation: true,
            max_remote_cost_usd: 0.000_001,
        },
    );

    let result = scheduler.decide(&StepContext::new("hello")).await;

    assert!(result.is_err());
}
