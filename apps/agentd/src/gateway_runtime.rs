//! Select the embedded service without changing an explicitly selected wire protocol.
use std::sync::Arc;

use agent_runtime::{EventListener, Runtime, RuntimeConfig, RuntimeTool, SessionPersistence};
use agent_secrets::ExposeSecret;

use crate::Daemon;

impl Daemon {
  pub(crate) fn gateway_contains(&self, model: &str) -> bool {
    self
      .config
      .gateway
      .models
      .iter()
      .any(|entry| entry.id == model)
      || self
        .config
        .gateway
        .groups
        .iter()
        .any(|entry| entry.id == model)
  }

  pub(crate) fn uses_gateway_registry(&self, binding: &agent_storage::ModelBinding) -> bool {
    model_provider::supports_gateway_chat(&binding.provider)
      && (binding.execution_id.starts_with("gateway:")
        || binding.execution_id == "configured" && self.gateway_contains(&binding.model))
  }

  pub(crate) async fn open_remote_runtime(
    &self,
    config: RuntimeConfig,
    persistence: Arc<dyn SessionPersistence>,
    tools: Vec<Arc<dyn RuntimeTool>>,
    listener: EventListener,
  ) -> agent_runtime::Result<Runtime> {
    // Responses, Gemini and Anthropic retain their native adapters. They must never be
    // silently encoded as Chat Completions by the embedded gateway.
    if !model_provider::supports_gateway_chat(&config.provider) {
      return Runtime::open(config, persistence, tools, listener).await;
    }
    let gateway = self
      .gateway
      .gateway_for(
        &config,
        self
          .config
          .gateway_registry
          .unwrap_or_else(|| self.gateway_contains(&config.model)),
      )
      .map_err(|_| {
        agent_runtime::RuntimeError::Configuration("embedded gateway model unavailable".into())
      })?;
    let model = model_provider::configured_with_gateway(
      &model_provider::ModelConfig {
        provider: config.provider.clone(),
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        system_prompt: config.system_prompt.clone(),
        context_window: config.context_window,
        max_output_tokens: config.max_output_tokens,
        supports_images: config.supports_images,
      },
      gateway,
      self.gateway.internal_token.expose_secret(),
    )
    .map_err(|_| {
      agent_runtime::RuntimeError::Configuration("embedded gateway model unavailable".into())
    })?;
    Runtime::open_with_model(config, persistence, tools, listener, model).await
  }
}
