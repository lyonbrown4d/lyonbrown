use std::sync::Arc;
use std::time::Duration;

use agent_app::{
  AgentCommand, ModelCatalogView, ModelPolicyView, ModelRuntimeView, RuntimePhase,
  spawn_runtime_with_models,
};
use agent_core::{Locality, ModelCapabilities, ModelTier, StopReason, TokenUsage};
use agent_kernel::Kernel;
use agent_model::{
  ModelError, ModelEvent, ModelProvider, ModelRegistry, ModelRequest, ModelStream,
};
use async_trait::async_trait;
use futures::stream;

struct BurstProvider {
  capabilities: ModelCapabilities,
}

#[async_trait]
impl ModelProvider for BurstProvider {
  fn provider_id(&self) -> &'static str {
    "burst"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(vec![self.capabilities.clone()])
  }

  async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
    let mut events = (0..600)
      .map(|_| Ok(ModelEvent::Delta("x".into())))
      .collect::<Vec<_>>();
    events.push(Ok(ModelEvent::Usage(TokenUsage::default())));
    events.push(Ok(ModelEvent::Done {
      stop_reason: StopReason::EndTurn,
    }));
    Ok(ModelStream {
      model: self.capabilities.model.clone(),
      events: Box::pin(stream::iter(events)),
    })
  }
}

fn capabilities() -> ModelCapabilities {
  ModelCapabilities {
    provider: "burst".into(),
    model: "worker".into(),
    tier: ModelTier::Worker,
    locality: Locality::Local,
    context_window: 32_768,
    max_output_tokens: 4_096,
    supports_tools: true,
    supports_streaming: true,
    supports_json: true,
    supports_thinking: false,
    cost_per_mtok_in: 0.0,
    cost_per_mtok_out: 0.0,
  }
}

#[tokio::test]
async fn broadcast_lag_replay_preserves_the_model_contract() {
  let capabilities = capabilities();
  let catalog = ModelCatalogView::try_from_capabilities([capabilities.clone()]).unwrap();
  let policy = ModelPolicyView::default();
  let models = ModelRuntimeView {
    catalog: Some(catalog.clone()),
    policy: policy.clone(),
  };
  let providers = ModelRegistry::new();
  providers
    .try_register(Arc::new(BurstProvider { capabilities }))
    .unwrap();
  let kernel = Kernel::builder().providers(providers).build();
  let (controller, runtime) = spawn_runtime_with_models(kernel, models, 4);
  let mut snapshots = controller.subscribe();

  controller
    .execute(AgentCommand::StartTurn {
      text: "trigger burst".into(),
    })
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    while snapshots.borrow().phase != RuntimePhase::Completed {
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .unwrap();

  let snapshot = snapshots.borrow().clone();
  assert_eq!(snapshot.turns[0].assistant_text.len(), 600);
  assert_eq!(snapshot.model_catalog, Some(catalog));
  assert_eq!(snapshot.model_policy, policy);
  drop(controller);
  runtime.join().await.unwrap();
}
