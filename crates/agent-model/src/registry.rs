use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_core::ModelCapabilities;
use futures::{StreamExt, future::BoxFuture, stream::FuturesUnordered};

use crate::{ModelError, ModelProvider};

mod catalog;

use catalog::{
  capabilities, extend_selected, probe_provider_catalog, validate_catalog, validate_provider_ids,
};

#[derive(Default)]
struct RegistryState {
  providers: Vec<Arc<dyn ModelProvider>>,
  sealed: Option<Vec<SelectedModel>>,
  sealing: bool,
}

#[derive(Clone, Default)]
pub struct ModelRegistry {
  state: Arc<Mutex<RegistryState>>,
}

impl ModelRegistry {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn register(&self, provider: Arc<dyn ModelProvider>) {
    self
      .try_register(provider)
      .expect("model providers must be registered before the registry is sealed");
  }

  pub fn try_register(&self, provider: Arc<dyn ModelProvider>) -> Result<(), RegistryError> {
    let mut state = self.lock_state();
    if state.sealing || state.sealed.is_some() {
      return Err(RegistryError::Sealed);
    }
    state.providers.push(provider);
    Ok(())
  }

  pub fn is_empty(&self) -> bool {
    self.lock_state().providers.is_empty()
  }

  pub async fn first_available(&self) -> Result<SelectedModel, RegistryError> {
    if let Some(models) = self.sealed_models() {
      return models.into_iter().next().ok_or(RegistryError::NoModels);
    }
    let providers = self.providers_snapshot()?;
    let mut last_error = None;
    for provider in providers {
      match provider.list_models().await {
        Ok(models) => {
          if let Some(capabilities) = models.into_iter().next() {
            return Ok(SelectedModel {
              provider,
              capabilities,
            });
          }
        }
        Err(error) => last_error = Some(error),
      }
    }
    match last_error {
      Some(error) => Err(RegistryError::Provider(error)),
      None => Err(RegistryError::NoModels),
    }
  }

  pub async fn available_models(&self) -> Result<Vec<SelectedModel>, RegistryError> {
    if let Some(models) = self.sealed_models() {
      return Ok(models);
    }
    let providers = self.providers_snapshot()?;

    let mut last_error = None;
    let mut available = Vec::new();
    for provider in providers {
      match provider.list_models().await {
        Ok(models) => {
          extend_selected(&mut available, provider, models);
        }
        Err(error) => last_error = Some(error),
      }
    }

    if !available.is_empty() {
      return Ok(available);
    }
    match last_error {
      Some(error) => Err(RegistryError::Provider(error)),
      None => Err(RegistryError::NoModels),
    }
  }

  /// Probe every provider concurrently, validate the complete catalog, then
  /// freeze the registry so routing observes this exact immutable snapshot.
  pub async fn seal_catalog(
    &self,
    provider_timeout: Duration,
  ) -> Result<Vec<ModelCapabilities>, RegistryError> {
    if let Some(models) = self.begin_seal()? {
      return Ok(capabilities(&models));
    }
    let mut attempt = SealAttempt::new(self.clone());
    let providers = self.providers_snapshot()?;
    validate_provider_ids(&providers)?;
    let mut probes: FuturesUnordered<
      BoxFuture<'static, Result<Vec<SelectedModel>, RegistryError>>,
    > = FuturesUnordered::new();
    for provider in providers {
      probes.push(Box::pin(probe_provider_catalog(provider, provider_timeout)));
    }
    let mut selected = Vec::new();
    while let Some(result) = probes.next().await {
      selected.extend(result?);
    }
    validate_catalog(&mut selected)?;
    let result = capabilities(&selected);
    attempt.commit(selected);
    Ok(result)
  }

  pub async fn catalog_models(
    &self,
    provider_timeout: Duration,
  ) -> Result<Vec<ModelCapabilities>, RegistryError> {
    self.seal_catalog(provider_timeout).await
  }

  fn begin_seal(&self) -> Result<Option<Vec<SelectedModel>>, RegistryError> {
    let mut state = self.lock_state();
    if let Some(models) = &state.sealed {
      return Ok(Some(models.clone()));
    }
    if state.sealing {
      return Err(RegistryError::Sealing);
    }
    if state.providers.is_empty() {
      return Err(RegistryError::Empty);
    }
    state.sealing = true;
    Ok(None)
  }

  fn sealed_models(&self) -> Option<Vec<SelectedModel>> {
    self.lock_state().sealed.clone()
  }

  fn providers_snapshot(&self) -> Result<Vec<Arc<dyn ModelProvider>>, RegistryError> {
    let providers = self.lock_state().providers.clone();
    if providers.is_empty() {
      Err(RegistryError::Empty)
    } else {
      Ok(providers)
    }
  }

  fn lock_state(&self) -> std::sync::MutexGuard<'_, RegistryState> {
    self
      .state
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
  }
}

struct SealAttempt {
  registry: ModelRegistry,
  committed: bool,
}

impl SealAttempt {
  fn new(registry: ModelRegistry) -> Self {
    Self {
      registry,
      committed: false,
    }
  }

  fn commit(&mut self, models: Vec<SelectedModel>) {
    let mut state = self.registry.lock_state();
    state.sealed = Some(models);
    state.sealing = false;
    self.committed = true;
  }
}

impl Drop for SealAttempt {
  fn drop(&mut self) {
    if !self.committed {
      self.registry.lock_state().sealing = false;
    }
  }
}

#[derive(Clone)]
pub struct SelectedModel {
  pub provider: Arc<dyn ModelProvider>,
  pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryErrorClass {
  Transient,
  Invariant,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
  #[error("model registry is empty")]
  Empty,
  #[error("registered providers expose no models")]
  NoModels,
  #[error(transparent)]
  Provider(#[from] ModelError),
  #[error("model catalog probe timed out for provider '{0}'")]
  CatalogTimeout(String),
  #[error("model catalog is already being sealed")]
  Sealing,
  #[error("model registry is sealed")]
  Sealed,
  #[error("duplicate registered model provider '{0}'")]
  DuplicateProvider(String),
  #[error("model provider identity mismatch: registered '{registered}', declared '{declared}'")]
  ProviderIdentity {
    registered: String,
    declared: String,
  },
  #[error("model catalog contains an invalid route")]
  InvalidRoute,
  #[error("model catalog contains a duplicate exact route")]
  DuplicateRoute,
  #[error("model catalog contains {0} entries and exceeds the limit")]
  CatalogOverflow(usize),
}

impl RegistryError {
  pub fn class(&self) -> RegistryErrorClass {
    match self {
      Self::NoModels | Self::Provider(_) | Self::CatalogTimeout(_) | Self::Sealing => {
        RegistryErrorClass::Transient
      }
      Self::Empty
      | Self::Sealed
      | Self::DuplicateProvider(_)
      | Self::ProviderIdentity { .. }
      | Self::InvalidRoute
      | Self::DuplicateRoute
      | Self::CatalogOverflow(_) => RegistryErrorClass::Invariant,
    }
  }
}
