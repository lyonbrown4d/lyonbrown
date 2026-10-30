use crate::Secret;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use subtle::ConstantTimeEq;

/// Server-owned identity and restrictions; never constructed from request metadata.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AccessPrincipal {
  pub key_id: String,
  pub project_id: String,
  pub allowed_models: BTreeSet<String>,
  pub max_output_tokens: Option<u64>,
}
impl AccessPrincipal {
  pub fn legacy() -> Self {
    Self {
      key_id: "legacy".into(),
      project_id: "default".into(),
      allowed_models: BTreeSet::new(),
      max_output_tokens: None,
    }
  }
  pub fn permits_model(&self, model: &str) -> bool {
    self.allowed_models.is_empty() || self.allowed_models.contains(model)
  }
}

#[derive(Debug, thiserror::Error)]
#[error("access verification unavailable")]
pub struct AccessError;

/// Implementations verify the server-owned credential store; request data cannot grant scope.
#[async_trait]
pub trait AccessControl: Send + Sync {
  async fn authorize(&self, bearer: &str) -> Result<bool, AccessError>;
  async fn authenticate(&self, bearer: &str) -> Result<Option<AccessPrincipal>, AccessError> {
    Ok(self.authorize(bearer).await?.then(AccessPrincipal::legacy))
  }
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
