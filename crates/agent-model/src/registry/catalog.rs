use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use agent_core::{
  MAX_MODEL_CATALOG_SIZE, MAX_MODEL_ID_BYTES, MAX_MODEL_PROVIDER_BYTES, ModelCapabilities,
};

use super::{RegistryError, SelectedModel};
use crate::ModelProvider;

pub(super) async fn probe_provider_catalog(
  provider: Arc<dyn ModelProvider>,
  provider_timeout: Duration,
) -> Result<Vec<SelectedModel>, RegistryError> {
  let provider_id = provider.provider_id().to_owned();
  let models = tokio::time::timeout(provider_timeout, provider.list_models())
    .await
    .map_err(|_| RegistryError::CatalogTimeout(provider_id.clone()))??;
  for model in &models {
    if model.provider != provider_id {
      return Err(RegistryError::ProviderIdentity {
        registered: provider_id,
        declared: model.provider.clone(),
      });
    }
  }
  let mut selected = Vec::with_capacity(models.len());
  extend_selected(&mut selected, provider, models);
  Ok(selected)
}

pub(super) fn validate_provider_ids(
  providers: &[Arc<dyn ModelProvider>],
) -> Result<(), RegistryError> {
  let mut ids = HashSet::with_capacity(providers.len());
  for provider in providers {
    let provider_id = provider.provider_id();
    if !valid_part(provider_id, MAX_MODEL_PROVIDER_BYTES) {
      return Err(RegistryError::ProviderIdentity {
        registered: provider_id.into(),
        declared: provider_id.into(),
      });
    }
    if !ids.insert(provider_id) {
      return Err(RegistryError::DuplicateProvider(provider_id.into()));
    }
  }
  Ok(())
}

pub(super) fn validate_catalog(models: &mut [SelectedModel]) -> Result<(), RegistryError> {
  if models.is_empty() {
    return Err(RegistryError::NoModels);
  }
  if models.len() > MAX_MODEL_CATALOG_SIZE {
    return Err(RegistryError::CatalogOverflow(models.len()));
  }
  for model in models.iter() {
    if !valid_part(&model.capabilities.provider, MAX_MODEL_PROVIDER_BYTES)
      || !valid_part(&model.capabilities.model, MAX_MODEL_ID_BYTES)
    {
      return Err(RegistryError::InvalidRoute);
    }
  }
  models.sort_by(|left, right| {
    (&left.capabilities.provider, &left.capabilities.model)
      .cmp(&(&right.capabilities.provider, &right.capabilities.model))
  });
  if models.windows(2).any(|pair| {
    pair[0].capabilities.provider == pair[1].capabilities.provider
      && pair[0].capabilities.model == pair[1].capabilities.model
  }) {
    return Err(RegistryError::DuplicateRoute);
  }
  Ok(())
}

fn valid_part(value: &str, max_bytes: usize) -> bool {
  !value.trim().is_empty() && value.len() <= max_bytes
}

pub(super) fn extend_selected(
  selected: &mut Vec<SelectedModel>,
  provider: Arc<dyn ModelProvider>,
  models: Vec<ModelCapabilities>,
) {
  selected.extend(models.into_iter().map(|capabilities| SelectedModel {
    provider: provider.clone(),
    capabilities,
  }));
}

pub(super) fn capabilities(models: &[SelectedModel]) -> Vec<ModelCapabilities> {
  models
    .iter()
    .map(|model| model.capabilities.clone())
    .collect()
}
