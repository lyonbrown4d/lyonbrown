use std::time::Duration;

use rig_core::providers::{anthropic::AnthropicConfig, openai::OpenAIConfig};
use rig_core::{DynModel, operation::Completion};

use crate::{ModelConfig, ProviderError, Result};

/// Connect an explicitly selected protocol with daemon-supplied credentials.
/// No environment credential discovery or model retry policy is installed.
pub fn configured(config: &ModelConfig) -> Result<DynModel<Completion>> {
  agent_security::initialize_tls();
  validate(config)?;
  let default_url = match config.provider.as_str() {
    "openai" | "openai-responses" | "openai-completions" => "https://api.openai.com/v1",
    "anthropic" => "https://api.anthropic.com",
    _ => return Err(invalid("unsupported provider")),
  };
  if config.api_key.trim().is_empty() {
    return Err(invalid("the daemon must supply a provider API key"));
  }
  let url = config.base_url.as_deref().unwrap_or(default_url);
  let http = crate::configured_http(url, Duration::from_secs(60), 16 * 1024 * 1024)?;
  Ok(match config.provider.as_str() {
    "anthropic" => AnthropicConfig::new(config.api_key.clone())
      .with_base_url(url)
      .connect(http)
      .completion(&config.model)
      .erase(),
    "openai-completions" => OpenAIConfig::new(config.api_key.clone())
      .with_base_url(url)
      .connect(http)
      .chat(&config.model)
      .erase(),
    _ => OpenAIConfig::new(config.api_key.clone())
      .with_base_url(url)
      .connect(http)
      .responses(&config.model)
      .erase(),
  })
}

pub(crate) fn validate(config: &ModelConfig) -> Result<()> {
  if config.model.trim().is_empty() {
    return Err(invalid("model is required"));
  }
  if config.context_window == 0
    || config.max_output_tokens == 0
    || config.max_output_tokens >= config.context_window
  {
    return Err(invalid(
      "output token limit must be positive and smaller than the context window",
    ));
  }
  Ok(())
}

fn invalid(reason: &str) -> ProviderError {
  ProviderError::Configuration(reason.into())
}
