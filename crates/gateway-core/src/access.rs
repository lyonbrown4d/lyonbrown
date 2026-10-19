use crate::Secret;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Debug)]
pub struct AccessError;
impl std::fmt::Display for AccessError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("access verification unavailable")
  }
}
impl std::error::Error for AccessError {}

/// Implementations verify the server-owned credential store; request data cannot grant scope.
#[async_trait]
pub trait AccessControl: Send + Sync {
  async fn authorize(&self, bearer: &str) -> Result<bool, AccessError>;
}

pub(crate) struct StaticAccess(pub Secret);
#[async_trait]
impl AccessControl for StaticAccess {
  async fn authorize(&self, bearer: &str) -> Result<bool, AccessError> {
    let expected = Sha256::digest(self.0.expose_secret().as_bytes());
    let actual = Sha256::digest(bearer.as_bytes());
    Ok(bool::from(expected.ct_eq(&actual)))
  }
}
