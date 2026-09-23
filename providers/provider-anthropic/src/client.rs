use std::sync::Arc;
use std::time::Duration;
use std::{net::IpAddr, str::FromStr};

use agent_core::{Locality, ModelCapabilities, ModelTier, SecretRef};
use agent_model::{ModelError, ModelProvider, ModelRequest, ModelStream};
use agent_secret::SecretBroker;
use async_trait::async_trait;
use reqwest::{Client, Url};

use crate::stream::into_model_stream;
use crate::wire::to_wire_request;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

pub struct AnthropicProvider {
  client: Client,
  messages_url: Url,
  api_key: ApiKey,
  models: Vec<ModelCapabilities>,
}

enum ApiKey {
  Literal(String),
  Reference(Arc<dyn SecretBroker>, SecretRef),
}

impl ApiKey {
  async fn resolve(&self) -> Result<String, ModelError> {
    match self {
      Self::Literal(value) => Ok(value.clone()),
      Self::Reference(broker, reference) => broker
        .resolve(reference)
        .await
        .map_err(|error| ModelError::Other(error.to_string())),
    }
  }
}

impl AnthropicProvider {
  pub fn new(
    api_key: impl Into<String>,
    models: Vec<ModelCapabilities>,
  ) -> Result<Self, ModelError> {
    Self::with_base_url(api_key, DEFAULT_BASE_URL, models)
  }

  pub fn with_base_url(
    api_key: impl Into<String>,
    base_url: impl AsRef<str>,
    models: Vec<ModelCapabilities>,
  ) -> Result<Self, ModelError> {
    let api_key = api_key.into();
    if api_key.trim().is_empty() {
      return Err(ModelError::Protocol(
        "Anthropic API key must not be empty".into(),
      ));
    }
    Self::with_key_source(ApiKey::Literal(api_key), base_url, models)
  }

  pub fn with_secret_ref(
    broker: Arc<dyn SecretBroker>,
    reference: SecretRef,
    base_url: impl AsRef<str>,
    models: Vec<ModelCapabilities>,
  ) -> Result<Self, ModelError> {
    Self::with_key_source(ApiKey::Reference(broker, reference), base_url, models)
  }

  pub fn with_default_secret_ref(
    broker: Arc<dyn SecretBroker>,
    reference: SecretRef,
  ) -> Result<Self, ModelError> {
    Self::with_secret_ref(broker, reference, DEFAULT_BASE_URL, default_models())
  }

  pub fn with_secret_ref_default_models(
    broker: Arc<dyn SecretBroker>,
    reference: SecretRef,
    base_url: impl AsRef<str>,
  ) -> Result<Self, ModelError> {
    Self::with_secret_ref(broker, reference, base_url, default_models())
  }

  fn with_key_source(
    api_key: ApiKey,
    base_url: impl AsRef<str>,
    models: Vec<ModelCapabilities>,
  ) -> Result<Self, ModelError> {
    let mut normalized = base_url.as_ref().trim_end_matches('/').to_owned();
    normalized.push('/');
    let base_url = Url::parse(&normalized)
      .map_err(|error| ModelError::Protocol(format!("invalid Anthropic URL: {error}")))?;
    validate_endpoint(&base_url)?;
    let messages_url = base_url
      .join("v1/messages")
      .map_err(|error| ModelError::Protocol(format!("invalid Anthropic messages URL: {error}")))?;
    let client = Client::builder()
      .timeout(REQUEST_TIMEOUT)
      .build()
      .map_err(|error| ModelError::Other(error.to_string()))?;
    Ok(Self {
      client,
      messages_url,
      api_key,
      models,
    })
  }

  pub fn with_default_models(api_key: impl Into<String>) -> Result<Self, ModelError> {
    Self::new(api_key, default_models())
  }
}

fn validate_endpoint(endpoint: &Url) -> Result<(), ModelError> {
  if !endpoint.username().is_empty() || endpoint.password().is_some() {
    return Err(ModelError::Protocol(
      "Anthropic endpoint must not contain URL userinfo".into(),
    ));
  }
  if !matches!(endpoint.scheme(), "http" | "https") {
    return Err(ModelError::Protocol(
      "Anthropic endpoint scheme must be HTTP or HTTPS".into(),
    ));
  }
  let host = endpoint
    .host_str()
    .ok_or_else(|| ModelError::Protocol("Anthropic endpoint must include a host".into()))?;
  let normalized_host = host.trim_matches(['[', ']']);
  let is_loopback = normalized_host.eq_ignore_ascii_case("localhost")
    || IpAddr::from_str(normalized_host).is_ok_and(|address| address.is_loopback());
  if endpoint.scheme() == "http" && !is_loopback {
    return Err(ModelError::Protocol(
      "Anthropic endpoint must use HTTPS unless its host is loopback".into(),
    ));
  }
  Ok(())
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
  fn provider_id(&self) -> &'static str {
    "anthropic"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    Ok(self.models.clone())
  }

  async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
    let capabilities = self
      .models
      .iter()
      .find(|candidate| candidate.model == request.model)
      .ok_or_else(|| {
        ModelError::Protocol(format!(
          "Anthropic model '{}' is not configured",
          request.model
        ))
      })?;
    if !request.tools.is_empty() && !capabilities.supports_tools {
      return Err(ModelError::Protocol(format!(
        "Anthropic model '{}' does not support tools",
        request.model
      )));
    }
    if request.json_mode && !capabilities.supports_json {
      return Err(ModelError::Protocol(format!(
        "Anthropic model '{}' does not support JSON mode",
        request.model
      )));
    }

    let model = request.model.clone();
    let api_key = self.api_key.resolve().await?;
    let response = self
      .client
      .post(self.messages_url.clone())
      .header("x-api-key", api_key)
      .header("anthropic-version", ANTHROPIC_VERSION)
      .header("accept", "text/event-stream")
      .json(&to_wire_request(request))
      .send()
      .await
      .map_err(map_reqwest_error)?;
    let status = response.status();
    if !status.is_success() {
      let body = response.text().await.map_err(map_reqwest_error)?;
      return Err(ModelError::Http(status.as_u16(), body));
    }

    Ok(ModelStream {
      model,
      events: into_model_stream(response),
    })
  }
}

pub(crate) fn map_reqwest_error(error: reqwest::Error) -> ModelError {
  if error.is_timeout() {
    ModelError::Timeout
  } else if error.is_connect() {
    ModelError::Connection(error.to_string())
  } else {
    ModelError::Other(error.to_string())
  }
}

fn default_models() -> Vec<ModelCapabilities> {
  vec![
    ModelCapabilities {
      provider: "anthropic".into(),
      model: "claude-sonnet-4-5".into(),
      tier: ModelTier::Frontier,
      locality: Locality::Remote,
      context_window: 200_000,
      max_output_tokens: 64_000,
      supports_tools: true,
      supports_streaming: true,
      supports_json: true,
      supports_thinking: true,
      cost_per_mtok_in: 3.0,
      cost_per_mtok_out: 15.0,
    },
    ModelCapabilities {
      provider: "anthropic".into(),
      model: "claude-haiku-4-5".into(),
      tier: ModelTier::Worker,
      locality: Locality::Remote,
      context_window: 200_000,
      max_output_tokens: 64_000,
      supports_tools: true,
      supports_streaming: true,
      supports_json: true,
      supports_thinking: false,
      cost_per_mtok_in: 1.0,
      cost_per_mtok_out: 5.0,
    },
  ]
}
