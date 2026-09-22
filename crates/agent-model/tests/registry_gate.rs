use std::pin::Pin;
use std::sync::Arc;

use agent_core::{Locality, ModelCapabilities, ModelTier, StepOutcome, StopReason, TokenUsage};
use agent_model::{
    GateVerdict, HeuristicGate, ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest,
    ModelStream, QualityGate,
};
use async_trait::async_trait;
use futures::{Stream, stream};

struct TestProvider;

#[async_trait]
impl ModelProvider for TestProvider {
    fn provider_id(&self) -> &'static str {
        "test"
    }

    async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
        Ok(vec![ModelCapabilities {
            provider: "test".into(),
            model: "test-model".into(),
            tier: ModelTier::Worker,
            locality: Locality::Local,
            context_window: 8_192,
            max_output_tokens: 1_024,
            supports_tools: true,
            supports_streaming: true,
            supports_json: true,
            supports_thinking: false,
            cost_per_mtok_in: 0.0,
            cost_per_mtok_out: 0.0,
        }])
    }

    async fn stream(&self, _req: ModelRequest) -> Result<ModelStream, ModelError> {
        let events: Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send>> =
            Box::pin(stream::empty());
        Ok(ModelStream {
            model: "test-model".into(),
            events,
        })
    }
}

#[tokio::test]
async fn registry_selects_the_first_available_model() {
    let registry = ModelRegistry::new();
    registry.register(Arc::new(TestProvider));

    let selected = registry.first_available().await.unwrap();

    assert_eq!(selected.provider.provider_id(), "test");
    assert_eq!(selected.capabilities.model, "test-model");
}

#[test]
fn heuristic_gate_escalates_empty_final_answer() {
    let verdict = HeuristicGate.evaluate(
        &StepOutcome {
            stop_reason: StopReason::EndTurn,
            text: "   ".into(),
            tool_calls: vec![],
            escalated: false,
        },
        &ModelCapabilities {
            provider: "test".into(),
            model: "test-model".into(),
            tier: ModelTier::Worker,
            locality: Locality::Local,
            context_window: 8_192,
            max_output_tokens: 1_024,
            supports_tools: true,
            supports_streaming: true,
            supports_json: true,
            supports_thinking: false,
            cost_per_mtok_in: 0.0,
            cost_per_mtok_out: 0.0,
        },
    );

    assert!(matches!(verdict, GateVerdict::Escalate(_)));
}

#[test]
fn heuristic_gate_accepts_non_empty_final_answer() {
    let outcome = StepOutcome {
        stop_reason: StopReason::EndTurn,
        text: "done".into(),
        tool_calls: vec![],
        escalated: false,
    };

    assert_eq!(
        HeuristicGate.evaluate(
            &outcome,
            &ModelCapabilities {
                provider: "test".into(),
                model: "test-model".into(),
                tier: ModelTier::Worker,
                locality: Locality::Local,
                context_window: 8_192,
                max_output_tokens: 1_024,
                supports_tools: true,
                supports_streaming: true,
                supports_json: true,
                supports_thinking: false,
                cost_per_mtok_in: 0.0,
                cost_per_mtok_out: 0.0,
            },
        ),
        GateVerdict::Pass
    );

    let _usage = TokenUsage::default();
}
