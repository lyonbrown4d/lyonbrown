use std::time::Duration;

use rig_core::{
  DynModel,
  catalog::Catalog,
  operation::Completion,
  providers::{
    openai::Route,
    registry::{ProviderConfig, ProviderId},
  },
};

use crate::{ModelConfig, ProviderError, Result};

/// Connect a framework-native provider using daemon-supplied credentials.
/// Registry presets supply each vendor's wire semantics; no SDK environment lookup occurs.
pub fn configured(config: &ModelConfig) -> Result<DynModel<Completion>> {
  validate(config)?;
  let provider = selection(
    &config.provider,
    &config.api_key,
    config.base_url.as_deref(),
  )?;
  let id = provider
    .id()
    .ok_or_else(|| invalid("unregistered provider"))?;
  let http = crate::configured_http(
    provider.base_url(),
    Duration::from_secs(60),
    16 * 1024 * 1024,
  )?;
  Ok(provider.completion_model(
    &config.model,
    http,
    Catalog::builtin().facts(id, &config.model),
  ))
}

/// Probe a configured provider through Rig's own model-list operation.
/// Completion encoding and model-list response decoding remain framework-owned.
pub async fn list_models(
  provider: &str,
  credential: &str,
  base_url: Option<&str>,
) -> Result<Vec<String>> {
  let provider = selection(provider, credential, base_url)?;
  let http = crate::configured_http(provider.base_url(), Duration::from_secs(5), 1024 * 1024)?;
  let operation = async {
    match provider {
      ProviderConfig::OpenAi(config) => config.connect(http).list_models().await,
      ProviderConfig::Anthropic(config) => config.connect(http).list_models().await,
      ProviderConfig::Gemini(config) => config.connect(http).list_models().await,
    }
  };
  let models = tokio::time::timeout(Duration::from_secs(5), operation)
    .await
    .map_err(|_| ProviderError::Request("provider catalog timed out".into()))?
    .map_err(|_| ProviderError::Request("provider model catalog unavailable".into()))?;
  if models.len() > 1000 {
    return Err(ProviderError::Request(
      "provider model catalog exceeded limit".into(),
    ));
  }
  let mut ids = models
    .data
    .into_iter()
    .map(|model| model.id)
    .collect::<Vec<_>>();
  if ids
    .iter()
    .any(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
  {
    return Err(ProviderError::Request(
      "provider returned an invalid model identifier".into(),
    ));
  }
  ids.sort();
  ids.dedup();
  Ok(ids)
}

fn selection(provider: &str, credential: &str, base_url: Option<&str>) -> Result<ProviderConfig> {
  if credential.trim().is_empty() || credential.chars().any(char::is_control) {
    return Err(invalid("the daemon must supply a valid provider API key"));
  }
  let vendor = match provider {
    "openai-responses" | "openai-completions" => "openai",
    "gemini-generate-content" => "gemini",
    value => value,
  };
  let mut selected = ProviderId::catalog(vendor)
    .and_then(|id| id.config(credential.to_owned()))
    .ok_or_else(|| invalid("unsupported provider"))?;
  if let ProviderConfig::OpenAi(config) = selected {
    selected = ProviderConfig::OpenAi(match provider {
      "openai" | "openai-responses" => config.with_route(Route::Responses),
      "openai-completions" => config.with_route(Route::Chat),
      _ => config,
    });
  }
  if let Some(url) = base_url {
    selected = selected.with_base_url(url);
  }
  Ok(selected)
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
