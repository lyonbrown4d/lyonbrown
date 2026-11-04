use super::GatewayHost;
use agent_secrets::ExposeSecret;
use anyhow::{Context, Result, ensure};
use decision_core::ModelTier;
use gateway_core::{Gateway, GatewayConfig, ModelConfig, Secret};
use sha2::{Digest, Sha256};
use std::sync::Arc;

impl GatewayHost {
  pub(crate) fn gateway_for(
    &self,
    runtime: &agent_runtime::RuntimeConfig,
    use_registry: bool,
  ) -> Result<Gateway> {
    ensure!(
      model_provider::supports_gateway_chat(&runtime.provider),
      "gateway requires Chat Completions"
    );
    if self.explicit && use_registry {
      return self
        .gateway
        .clone()
        .context("configured gateway unavailable");
    }
    let default_base = model_provider::provider_base_url(&runtime.provider)?;
    let base = runtime.base_url.as_deref().unwrap_or(&default_base);
    let key = cache_key(
      &runtime.model,
      base,
      &runtime.api_key,
      runtime.context_window,
      runtime.supports_images,
    );
    let mut cache = self
      .cache
      .lock()
      .map_err(|_| anyhow::anyhow!("gateway cache unavailable"))?;
    if let Some(gateway) = cache.get(&key) {
      return Ok(gateway.clone());
    }
    ensure!(cache.len() < 256, "gateway model cache capacity reached");
    let config = GatewayConfig {
      token: Secret::new(self.internal_token.expose_secret().to_owned()),
      models: vec![ModelConfig {
        id: runtime.model.clone(),
        upstream_model: runtime.model.clone(),
        api_base: base.to_owned(),
        api_key: (!runtime.api_key.is_empty()).then(|| Secret::new(runtime.api_key.clone())),
        api_key_env: None,
        tier: ModelTier::Balanced,
        local_inference: false,
        capabilities: ["text".to_owned(), "tools".to_owned()]
          .into_iter()
          .chain(runtime.supports_images.then(|| "vision".to_owned()))
          .collect(),
        context_tokens: Some(u64::from(runtime.context_window)),
      }],
      endpoints: vec![],
      groups: vec![],
      limits: Default::default(),
      constraints: Default::default(),
    };
    let gateway = Gateway::new_with_access_and_privacy(
      config,
      self.decision.clone(),
      Arc::new(self.storage.clone()),
      self.access.clone(),
      self.privacy.clone(),
    )?;
    cache.insert(key, gateway.clone());
    Ok(gateway)
  }
}

pub(super) fn cache_key(
  model: &str,
  base: &str,
  credential: &str,
  context_window: u32,
  supports_images: bool,
) -> [u8; 32] {
  let mut hash = Sha256::new();
  for value in [model, base, credential] {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value.as_bytes());
  }
  hash.update(context_window.to_be_bytes());
  hash.update([u8::from(supports_images)]);
  hash.finalize().into()
}
