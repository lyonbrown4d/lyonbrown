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
        self.providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(provider);
    }

    pub fn is_empty(&self) -> bool {
        self.providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    pub async fn first_available(&self) -> Result<SelectedModel, RegistryError> {
        let providers = self
            .providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        if providers.is_empty() {
            return Err(RegistryError::Empty);
        }

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
