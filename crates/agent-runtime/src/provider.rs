use crate::{Result, RuntimeConfig, RuntimeError};
use rpi_ai::{Model, Provider};
use std::sync::Arc;

pub(crate) fn model_config(config: &RuntimeConfig) -> model_provider::ModelConfig {
  model_provider::ModelConfig {
    provider: config.provider.clone(),
    model: config.model.clone(),
    api_key: config.api_key.clone(),
    base_url: config.base_url.clone(),
    system_prompt: config.system_prompt.clone(),
    context_window: config.context_window,
    max_output_tokens: config.max_output_tokens,
    supports_images: config.supports_images,
  }
}
pub(crate) fn configured(config: &RuntimeConfig) -> Result<(Model, Arc<dyn Provider>)> {
  if config.session_id.trim().is_empty() || config.owner_epoch == 0 {
    return Err(RuntimeError::Configuration(
      "session and a positive owner epoch are required".into(),
    ));
  }
  model_provider::configured(&model_config(config))
    .map_err(|error| RuntimeError::Configuration(error.to_string()))
}
