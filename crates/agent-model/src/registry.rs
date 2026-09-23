use std::sync::{Arc, Mutex};

use agent_core::ModelCapabilities;

use crate::{ModelError, ModelProvider};

#[derive(Clone, Default)]
pub struct ModelRegistry {
  providers: Arc<Mutex<Vec<Arc<dyn ModelProvider>>>>,
}

impl ModelRegistry {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn register(&self, provider: Arc<dyn ModelProvider>) {
    self
      .providers
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .push(provider);
  }

  pub fn is_empty(&self) -> bool {
    self
      .providers
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .is_empty()
  }

  pub async fn first_available(&self) -> Result<SelectedModel, RegistryError> {
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
    let providers = self.providers_snapshot()?;

    let mut last_error = None;
    let mut available = Vec::new();
    for provider in providers {
      match provider.list_models().await {
        Ok(models) => {
          for capabilities in models {
            available.push(SelectedModel {
              provider: provider.clone(),
              capabilities,
            });
          }
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

  fn providers_snapshot(&self) -> Result<Vec<Arc<dyn ModelProvider>>, RegistryError> {
    let providers = self
      .providers
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .clone();
    if providers.is_empty() {
      Err(RegistryError::Empty)
    } else {
      Ok(providers)
    }
  }
}

pub struct SelectedModel {
  pub provider: Arc<dyn ModelProvider>,
  pub capabilities: ModelCapabilities,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
  #[error("model registry is empty")]
  Empty,
  #[error("registered providers expose no models")]
  NoModels,
  #[error(transparent)]
  Provider(#[from] ModelError),
}
