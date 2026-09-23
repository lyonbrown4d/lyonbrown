use agent_core::SecretRef;
use async_trait::async_trait;

use crate::redact::ResolvedSecrets;
use crate::{SecretBroker, SecretError};

#[derive(Clone, Default)]
pub struct EnvSecretBroker {
  resolved: ResolvedSecrets,
}

impl EnvSecretBroker {
  pub fn new() -> Self {
    Self::default()
  }
}

#[async_trait]
impl SecretBroker for EnvSecretBroker {
  async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError> {
    let (kind, name) = reference.parts();
    if kind != "env" {
      return Err(SecretError::NotFound(reference.clone()));
    }
    if name.is_empty() || name.contains(['=', '\0']) {
      return Err(SecretError::InvalidReference(reference.clone()));
    }
    let value = std::env::var(name).map_err(|error| match error {
      std::env::VarError::NotPresent => SecretError::NotFound(reference.clone()),
      std::env::VarError::NotUnicode(_) => SecretError::Provider(format!(
        "environment secret is not valid Unicode: {reference}"
      )),
    })?;
    self.resolved.register(&value);
    Ok(value)
  }

  fn redact(&self, text: &str) -> String {
    self.resolved.redact(text)
  }
}
