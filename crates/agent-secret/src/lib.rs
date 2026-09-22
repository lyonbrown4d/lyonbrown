//! `agent-secret` — SecretBroker：延迟绑定 + 输出 redaction（§18-23）。
//!
//! M1 只提供运行时接缝与 [`NoopBroker`]；真实 secret provider 在 M5 实现。

use agent_core::SecretRef;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret not found: {0}")]
    NotFound(SecretRef),
    #[error("secret provider error: {0}")]
    Provider(String),
}

#[async_trait]
pub trait SecretBroker: Send + Sync {
    async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError>;

    async fn env_for(
        &self,
        references: &[SecretRef],
    ) -> Result<Vec<(String, String)>, SecretError> {
        let mut values = Vec::with_capacity(references.len());
        for reference in references {
            values.push((reference.to_string(), self.resolve(reference).await?));
        }
        Ok(values)
    }

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
