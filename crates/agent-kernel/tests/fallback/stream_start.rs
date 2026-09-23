use super::*;

struct StreamStartTimeout;

#[async_trait]
impl ModelProvider for StreamStartTimeout {
  fn provider_id(&self) -> &'static str {
    "local"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Provider {
      id: "local",
      locality: Locality::Local,
      tier: ModelTier::Worker,
      failure: None,
    }
    .list_models()
    .await
  }

  async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
    Ok(ModelStream {
      model: request.model,
      events: Box::pin(stream::iter(vec![Err(ModelError::Timeout)])),
    })
  }
}

#[tokio::test]
async fn stream_error_before_first_output_retries_remote() {
  let registry = ModelRegistry::new();
  registry.register(Arc::new(StreamStartTimeout));
  registry.register(Arc::new(Provider {
    id: "remote",
    locality: Locality::Remote,
    tier: ModelTier::Frontier,
    failure: None,
  }));
  let mut kernel = Kernel::builder().providers(registry).build();
  let events = kernel.run_turn("help me").await.unwrap();
  assert_eq!(
    events.iter().find_map(|event| event.turn_answer()),
    Some("remote answer")
  );
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind() == "ModelFallback")
      .count(),
    1
  );
}
