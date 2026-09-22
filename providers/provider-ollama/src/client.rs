use std::time::Duration;
use std::{net::IpAddr, str::FromStr};

use agent_core::{Locality, ModelCapabilities, ModelTier};
use agent_model::{ModelError, ModelProvider, ModelRequest, ModelStream};
use async_trait::async_trait;
use reqwest::{Client, Url};

use crate::stream::{into_model_stream, to_wire_request};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub struct OllamaProvider {
    client: Client,
    chat_url: Url,
    models: Vec<ModelCapabilities>,
}

impl OllamaProvider {
    pub fn new(
        base_url: impl AsRef<str>,
        models: Vec<ModelCapabilities>,
    ) -> Result<Self, ModelError> {
        let mut normalized = base_url.as_ref().trim_end_matches('/').to_owned();
        normalized.push('/');
        let base_url = Url::parse(&normalized)
            .map_err(|error| ModelError::Protocol(format!("invalid Ollama URL: {error}")))?;
        validate_endpoint(&base_url)?;
        let chat_url = base_url
            .join("api/chat")
            .map_err(|error| ModelError::Protocol(format!("invalid Ollama chat URL: {error}")))?;
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| ModelError::Other(error.to_string()))?;
        Ok(Self {
            client,
            chat_url,
            models,
        })
    }

    pub fn with_default_models(base_url: impl AsRef<str>) -> Result<Self, ModelError> {
        Self::new(base_url, default_models())
    }
}

fn validate_endpoint(endpoint: &Url) -> Result<(), ModelError> {
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(ModelError::Protocol(
            "Ollama endpoint must not contain URL userinfo".into(),
        ));
    }
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(ModelError::Protocol(
            "Ollama endpoint scheme must be HTTP or HTTPS".into(),
        ));
    }
    let host = endpoint
        .host_str()
        .ok_or_else(|| ModelError::Protocol("Ollama endpoint must include a host".into()))?;
    let normalized_host = host.trim_matches(['[', ']']);
    let is_loopback = normalized_host.eq_ignore_ascii_case("localhost")
        || IpAddr::from_str(normalized_host).is_ok_and(|address| address.is_loopback());
    if endpoint.scheme() == "http" && !is_loopback {
        return Err(ModelError::Protocol(
            "Ollama endpoint must use HTTPS unless its host is loopback".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn provider_id(&self) -> &'static str {
        "ollama"
    }

    async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
        Ok(self.models.clone())
    }

    async fn stream(&self, mut request: ModelRequest) -> Result<ModelStream, ModelError> {
        let capabilities = self
            .models
            .iter()
            .find(|candidate| candidate.model == request.model)
            .ok_or_else(|| {
                ModelError::Protocol(format!(
                    "Ollama model '{}' is not configured",
                    request.model
                ))
            })?;
        if !request.tools.is_empty() && !capabilities.supports_tools {
            return Err(ModelError::Protocol(format!(
                "Ollama model '{}' does not support tools",
                request.model
            )));
        }
        if request.json_mode && !capabilities.supports_json {
            return Err(ModelError::Protocol(format!(
                "Ollama model '{}' does not support JSON mode",
                request.model
            )));
        }
        if capabilities.tier == ModelTier::Worker && capabilities.supports_thinking {
            request.thinking = Some(false);
        }

        let model = request.model.clone();
        let response = self
            .client
            .post(self.chat_url.clone())
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

fn map_reqwest_error(error: reqwest::Error) -> ModelError {
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
            provider: "ollama".into(),
            model: "qwen3.8-27b:q3-claude".into(),
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
    ]
}
