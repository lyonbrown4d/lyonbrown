use std::path::Path;
use std::sync::Arc;

use agent_core::SecretRef;
use async_trait::async_trait;

use crate::redact::ResolvedSecrets;
use crate::{EnvSecretBroker, FileSecretBroker, SecretBroker, SecretError};

pub struct CompositeBroker {
    brokers: Vec<Arc<dyn SecretBroker>>,
    resolved: ResolvedSecrets,
}

impl CompositeBroker {
    pub fn new(brokers: Vec<Arc<dyn SecretBroker>>) -> Self {
        Self {
            brokers,
            resolved: ResolvedSecrets::default(),
        }
    }

    pub fn for_home(home: &Path) -> Result<Self, SecretError> {
        Ok(Self::new(vec![
            Arc::new(EnvSecretBroker::new()),
            Arc::new(FileSecretBroker::new(home.join(".agent").join("secrets"))?),
        ]))
    }
}

#[async_trait]
impl SecretBroker for CompositeBroker {
    async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError> {
        for broker in &self.brokers {
            match broker.resolve(reference).await {
                Ok(value) => {
                    self.resolved.register(&value);
                    return Ok(value);
                }
                Err(SecretError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Err(SecretError::NotFound(reference.clone()))
    }

    fn redact(&self, text: &str) -> String {
        self.resolved.redact(text)
    }
}
