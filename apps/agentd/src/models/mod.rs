//! Daemon-owned loaded models. Configuration alone never advertises availability.
mod load;
use agent_inference::{InferenceBackend, ModelInfo, ModelRegistry};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct Models {
  registry: Arc<ModelRegistry>,
  metadata: BTreeMap<String, ModelInfo>,
}
impl Models {
  pub(crate) async fn load(config: &crate::config::Config) -> Result<Self> {
    agent_config::validate_local_models(&config.local_models)?;
    for id in selected(config) {
      ensure!(
        config.local_models.iter().any(|profile| profile.id == id),
        "selected local model has no configured profile"
      );
    }
    let registry = Arc::new(ModelRegistry::new(config.max_runs)?);
    // Validate every profile before allocating any native model.
    for profile in &config.local_models {
      load::validate(profile)?;
    }
    for profile in &config.local_models {
      let started = std::time::Instant::now();
      let backend = load::backend(profile).await?;
      registry
        .register(backend, profile.parallel_requests)
        .await?;
      tracing::info!(model_id = %profile.id, elapsed_ms = started.elapsed().as_millis() as u64,
        "local model loaded on CPU");
    }
    Self::from_registry(config, registry).await
  }
  pub(crate) async fn from_registry(
    config: &crate::config::Config,
    registry: Arc<ModelRegistry>,
  ) -> Result<Self> {
    let metadata: BTreeMap<_, _> = registry
      .models()
      .await
      .into_iter()
      .map(|info| (info.id.clone(), info))
      .collect();
    for profile in &config.local_models {
      let info = metadata
        .get(&profile.id)
        .ok_or_else(|| anyhow::anyhow!("configured local profile is not loaded"))?;
      ensure!(
        info.context_window == profile.context_window,
        "loaded local model context differs from configured profile"
      );
      ensure!(
        profile.max_output_tokens < info.context_window,
        "configured local output budget exceeds loaded context"
      );
    }
    for id in selected(config) {
      let info = metadata
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("configured local model is not loaded"))?;
      ensure!(
        info.context_window > 1,
        "selected local model context is unusable"
      );
    }
    Ok(Self { registry, metadata })
  }
  pub(crate) fn info(&self, id: &str) -> Result<&ModelInfo> {
    self
      .metadata
      .get(id)
      .ok_or_else(|| anyhow::anyhow!("local model is not loaded"))
  }
  pub(crate) fn all(&self) -> Vec<ModelInfo> {
    self.metadata.values().cloned().collect()
  }
  pub(crate) async fn backend(&self, id: &str) -> Result<Arc<dyn InferenceBackend>> {
    Ok(self.registry.backend(id).await?)
  }
}

fn selected(config: &crate::config::Config) -> Vec<&str> {
  let mut ids = Vec::new();
  if config.provider == "local" {
    ids.push(config.model.as_str());
  }
  if config
    .decision
    .provider
    .as_deref()
    .unwrap_or(&config.provider)
    == "local"
    && let Some(id) = &config.decision.model
  {
    ids.push(id);
  }
  ids
}
