use std::sync::Arc;
use std::time::Duration;

use rpi_ai::providers::anthropic::AnthropicProvider;
use rpi_ai::providers::openai_completions::OpenAiCompletionsProvider;
use rpi_ai::providers::openai_responses::OpenAiResponsesProvider;
use rpi_ai::{Api, Model, Provider};

use crate::{ModelConfig, ProviderError, Result};

pub fn configured(config: &ModelConfig) -> Result<(Model, Arc<dyn Provider>)> {
  agent_security::initialize_tls();
  if config.model.trim().is_empty() {
    return Err(ProviderError::Configuration("model is required".into()));
  }
  if config.api_key.trim().is_empty() {
    return Err(ProviderError::Configuration(
      "the daemon must supply a provider API key".into(),
    ));
  }
  if config.context_window == 0
    || config.max_output_tokens == 0
    || config.max_output_tokens >= config.context_window
  {
    return Err(ProviderError::Configuration(
      "output token limit must be positive and smaller than the context window".into(),
    ));
  }
  let (api, default_url, provider_id) = match config.provider.as_str() {
    "openai" | "openai-responses" => (Api::OpenaiResponses, "https://api.openai.com/v1", "openai"),
    "openai-completions" => (
      Api::OpenaiCompletions,
      "https://api.openai.com/v1",
      "openai",
    ),
    "anthropic" => (
      Api::AnthropicMessages,
      "https://api.anthropic.com",
      "anthropic",
    ),
    name => {
      return Err(ProviderError::Configuration(format!(
        "unsupported provider: {name}"
      )));
    }
  };
  let url = config.base_url.as_deref().unwrap_or(default_url);
  let endpoint =
    reqwest::Url::parse(url).map_err(|error| ProviderError::Configuration(error.to_string()))?;
  if !matches!(endpoint.scheme(), "http" | "https")
    || endpoint.host_str().is_none()
    || !endpoint.username().is_empty()
    || endpoint.password().is_some()
    || endpoint.query().is_some()
    || endpoint.fragment().is_some()
  {
    return Err(ProviderError::Configuration(
      "provider endpoint must be HTTP(S) with no embedded credentials".into(),
    ));
  }
  let host = endpoint
    .host_str()
    .unwrap_or_default()
    .trim_start_matches('[')
    .trim_end_matches(']');
  let loopback = host == "localhost"
    || host
      .parse::<std::net::IpAddr>()
      .is_ok_and(|ip| ip.is_loopback());
  if endpoint.scheme() == "http" && !loopback {
    return Err(ProviderError::Configuration(
      "remote provider endpoints require HTTPS".into(),
    ));
  }
  let http = reqwest::Client::builder()
    .connect_timeout(Duration::from_secs(15))
    .retry(reqwest::retry::never())
    .redirect(reqwest::redirect::Policy::none());
  // Local inference and mocks must not leave loopback through an environment proxy.
  // Preserve the existing remote-provider proxy configuration for remote endpoints.
  let http = if loopback { http.no_proxy() } else { http }
    .build()
    .map_err(|error| ProviderError::Configuration(error.to_string()))?;
  let mut model = Model::new(
    config.model.clone(),
    config.model.clone(),
    api.clone(),
    provider_id,
    url,
  );
  model.context_window = u64::from(config.context_window);
  model.max_tokens = u64::from(config.max_output_tokens);
  if config.supports_images {
    model.input.push(rpi_ai::InputModality::Image);
  }
  let models = vec![model.clone()];
  let key = Some(config.api_key.clone());
  let provider: Arc<dyn Provider> =
    match api {
      Api::AnthropicMessages => Arc::new(AnthropicProvider::with_models_without_env_api_key(
        key, http, models,
      )),
      Api::OpenaiCompletions => Arc::new(
        OpenAiCompletionsProvider::with_models_without_env_api_key(provider_id, key, http, models),
      ),
      _ => Arc::new(OpenAiResponsesProvider::with_models_without_env_api_key(
        provider_id,
        key,
        http,
        models,
      )),
    };
  Ok((model, provider))
}
