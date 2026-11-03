use std::sync::Arc;

use agent_inference::InferenceBackend;
use rig_core::completion::CompletionRequest;
use tokio_util::sync::CancellationToken;

use crate::{ModelConfig, ProviderError};

pub async fn classify_local_json(
  backend: &Arc<dyn InferenceBackend>,
  system_prompt: String,
  user_prompt: String,
  cancellation: CancellationToken,
) -> crate::Result<String> {
  let info = backend.info();
  if info.context_window < 2 {
    return Err(ProviderError::Request(
      "local classification unavailable".into(),
    ));
  }
  let max_output_tokens = 2048.min(info.context_window - 1);
  let config = ModelConfig {
    provider: info.backend,
    model: info.id,
    api_key: String::new(),
    base_url: None,
    system_prompt: Some(system_prompt.clone()),
    context_window: info.context_window,
    max_output_tokens,
    supports_images: info.supports_images,
  };
  let model = crate::local_model(&config, backend.clone())?;
  let request = CompletionRequest::new(user_prompt)
    .preamble(system_prompt)
    .max_tokens(u64::from(max_output_tokens))
    .temperature(0.0);
  crate::classification::classify(model, request, cancellation).await
}
