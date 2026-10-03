use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use agent_core::{Locality, ModelCapabilities, ModelTier};
use agent_model::{
  ModelError, ModelProvider, ModelRegistry, ModelRequest, ModelScheduler, ModelStream,
  RegistryError, RoutingPolicy, StepContext,
};
use async_trait::async_trait;

struct CatalogProvider {
  id: &'static str,
  capabilities: ModelCapabilities,
  behavior: Behavior,
}

struct ChangingProvider {
  calls: AtomicUsize,
}

struct ModelsProvider(Vec<ModelCapabilities>);

#[async_trait]
impl ModelProvider for ModelsProvider {
  fn provider_id(&self) -> &'static str {
    "multi"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(self.0.clone())
  }

  async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
    unreachable!()
  }
}

#[async_trait]
impl ModelProvider for ChangingProvider {
  fn provider_id(&self) -> &'static str {
    "changing"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
    Ok(vec![capabilities("changing", &format!("worker-{call}"))])
  }

  async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
    unreachable!()
  }
}

fn capabilities(provider: &str, model: &str) -> ModelCapabilities {
  ModelCapabilities {
    provider: provider.into(),
    model: model.into(),
    tier: ModelTier::Worker,
    locality: Locality::Local,
    context_window: 32_768,
    max_output_tokens: 4_096,
    supports_tools: true,
    supports_streaming: true,
    supports_json: true,
    supports_thinking: true,
    cost_per_mtok_in: 0.0,
    cost_per_mtok_out: 0.0,
  }
}

enum Behavior {
  Ready,
  Fails,
  Never,
}

#[async_trait]
impl ModelProvider for CatalogProvider {
  fn provider_id(&self) -> &'static str {
    self.id
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    match self.behavior {
      Behavior::Ready => Ok(vec![self.capabilities.clone()]),
      Behavior::Fails => Err(ModelError::Other("provider failed".into())),
      Behavior::Never => std::future::pending().await,
    }
  }

  async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
    unreachable!()
  }
}

fn provider(id: &'static str, declared_provider: &str, behavior: Behavior) -> Arc<CatalogProvider> {
  Arc::new(CatalogProvider {
    id,
    capabilities: ModelCapabilities {
      provider: declared_provider.into(),
      model: "worker".into(),
      tier: ModelTier::Worker,
      locality: Locality::Local,
      context_window: 32_768,
      max_output_tokens: 4_096,
      supports_tools: true,
      supports_streaming: true,
      supports_json: true,
      supports_thinking: true,
      cost_per_mtok_in: 0.0,
      cost_per_mtok_out: 0.0,
    },
    behavior,
  })
}

#[tokio::test]
async fn catalog_models_are_bounded_and_concurrent() {
  let registry = ModelRegistry::new();
  for index in 0..20 {
    let id = Box::leak(format!("local-{index}").into_boxed_str());
    registry
      .try_register(provider(id, id, Behavior::Never))
      .unwrap();
  }

  let result = tokio::time::timeout(
    Duration::from_millis(80),
    registry.catalog_models(Duration::from_millis(10)),
  )
  .await
  .expect("provider probes must run concurrently and remain bounded");

  assert!(matches!(result, Err(RegistryError::CatalogTimeout(_))));
}

#[tokio::test]
async fn catalog_models_reject_provider_identity_mismatch() {
  let registry = ModelRegistry::new();
  registry
    .try_register(provider("registered", "declared", Behavior::Ready))
    .unwrap();

  let result = registry.catalog_models(Duration::from_secs(1)).await;

  assert!(matches!(
    result,
    Err(RegistryError::ProviderIdentity { .. })
  ));
}

#[tokio::test]
async fn catalog_models_reject_empty_and_duplicate_provider_ids() {
  let empty = ModelRegistry::new();
  empty
    .try_register(provider("", "", Behavior::Ready))
    .unwrap();
  assert!(matches!(
    empty.catalog_models(Duration::from_secs(1)).await,
    Err(RegistryError::ProviderIdentity { .. })
  ));

  let duplicate = ModelRegistry::new();
  duplicate
    .try_register(provider("local", "local", Behavior::Ready))
    .unwrap();
  duplicate
    .try_register(provider("local", "local", Behavior::Ready))
    .unwrap();
  assert!(matches!(
    duplicate.catalog_models(Duration::from_secs(1)).await,
    Err(RegistryError::DuplicateProvider(_))
  ));
}

#[tokio::test]
async fn catalog_models_are_all_or_nothing_on_provider_error() {
  let registry = ModelRegistry::new();
  registry
    .try_register(provider("good", "good", Behavior::Ready))
    .unwrap();
  registry
    .try_register(provider("bad", "bad", Behavior::Fails))
    .unwrap();

  let result = registry.catalog_models(Duration::from_secs(1)).await;

  assert!(matches!(result, Err(RegistryError::Provider(_))));
}

#[tokio::test]
async fn catalog_models_return_only_declared_registry_capabilities() {
  let registry = ModelRegistry::new();
  registry
    .try_register(provider("local", "local", Behavior::Ready))
    .unwrap();

  let models = registry
    .catalog_models(Duration::from_secs(1))
    .await
    .unwrap();

  assert_eq!(models.len(), 1);
  assert_eq!(models[0].provider, "local");
  assert_eq!(models[0].model, "worker");
}

#[tokio::test]
async fn sealed_catalog_and_scheduler_share_one_immutable_snapshot() {
  let registry = ModelRegistry::new();
  let provider = Arc::new(ChangingProvider {
    calls: AtomicUsize::new(0),
  });
  registry.try_register(provider.clone()).unwrap();

  let catalog = registry.seal_catalog(Duration::from_secs(1)).await.unwrap();
  let scheduler = ModelScheduler::new(registry.clone(), RoutingPolicy::default());
  let candidates = scheduler
    .decide_ranked(&StepContext::new("work"))
    .await
    .unwrap();

  assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
  assert_eq!(catalog.len(), candidates.len());
  assert_eq!(catalog[0], candidates[0].capabilities);
}

#[tokio::test]
async fn sealed_registry_rejects_late_registration() {
  let registry = ModelRegistry::new();
  registry
    .try_register(provider("local", "local", Behavior::Ready))
    .unwrap();
  registry.seal_catalog(Duration::from_secs(1)).await.unwrap();

  let error = registry
    .try_register(provider("late", "late", Behavior::Ready))
    .unwrap_err();

  assert!(matches!(error, RegistryError::Sealed));
}

#[tokio::test]
async fn sealing_rejects_duplicate_oversized_and_over_limit_catalogs() {
  for (models, expected) in [
    (
      vec![capabilities("multi", "same"), capabilities("multi", "same")],
      RegistryError::DuplicateRoute,
    ),
    (
      vec![capabilities("multi", &"m".repeat(257))],
      RegistryError::InvalidRoute,
    ),
    (
      (0..129)
        .map(|index| capabilities("multi", &format!("model-{index}")))
        .collect(),
      RegistryError::CatalogOverflow(129),
    ),
  ] {
    let registry = ModelRegistry::new();
    registry
      .try_register(Arc::new(ModelsProvider(models)))
      .unwrap();
    let error = registry
      .seal_catalog(Duration::from_secs(1))
      .await
      .unwrap_err();
    assert_eq!(error.class(), expected.class());
    assert_eq!(
      std::mem::discriminant(&error),
      std::mem::discriminant(&expected)
    );
  }

  let registry = ModelRegistry::new();
  registry
    .try_register(Arc::new(ModelsProvider(vec![capabilities(
      "multi",
      &"m".repeat(256),
    )])))
    .unwrap();
  assert!(registry.seal_catalog(Duration::from_secs(1)).await.is_ok());
}
