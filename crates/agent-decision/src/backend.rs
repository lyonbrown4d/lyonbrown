use crate::{ClassificationRequest, ClassifierError, DecisionBackend};
use agent_inference::InferenceBackend;
use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct RemoteClassifier {
  config: agent_runtime::RuntimeConfig,
}
impl RemoteClassifier {
  pub fn new(config: agent_runtime::RuntimeConfig) -> Self {
    Self { config }
  }
}
#[async_trait]
impl DecisionBackend for RemoteClassifier {
  async fn classify(
    &self,
    request: ClassificationRequest,
    cancellation: CancellationToken,
  ) -> Result<String, ClassifierError> {
    let mut config = self.config.clone();
    config.system_prompt = Some(request.system_prompt);
    config.max_output_tokens = config.max_output_tokens.min(2048);
    agent_runtime::classify_json(&config, &request.user_prompt, cancellation)
      .await
      .map_err(|_| ClassifierError)
  }
}

/// Local backends expose the same bounded, tool-free classification contract.
pub struct LocalClassifier {
  backend: Arc<dyn InferenceBackend>,
}
impl LocalClassifier {
  pub fn new(backend: Arc<dyn InferenceBackend>) -> Self {
    Self { backend }
  }
}
#[async_trait]
impl DecisionBackend for LocalClassifier {
  async fn classify(
    &self,
    request: ClassificationRequest,
    cancellation: CancellationToken,
  ) -> Result<String, ClassifierError> {
    model_provider::classify_local_json(
      &self.backend,
      request.system_prompt,
      request.user_prompt,
      cancellation,
    )
    .await
    .map_err(|_| ClassifierError)
  }
}
