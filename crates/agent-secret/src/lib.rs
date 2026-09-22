//! `agent-secret` — SecretBroker：延迟绑定 + 输出 redaction（§18-23）。

mod composite;
mod env_broker;
mod file_broker;
mod redact;

pub use composite::CompositeBroker;
pub use env_broker::EnvSecretBroker;
pub use file_broker::FileSecretBroker;

use agent_core::SecretRef;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret not found: {0}")]
    NotFound(SecretRef),
    #[error("invalid secret reference: {0}")]
    InvalidReference(SecretRef),
    #[error("secret provider error: {0}")]
    Provider(String),
}

#[async_trait]
pub trait SecretBroker: Send + Sync {
    async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError>;

    fn redact(&self, text: &str) -> String {
        text.to_owned()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopBroker;

#[async_trait]
impl SecretBroker for NoopBroker {
    async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError> {
        Err(SecretError::NotFound(reference.clone()))
    }
}
