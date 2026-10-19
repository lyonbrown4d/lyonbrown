use crate::{DecisionOutput, DecisionRequest, PolicySnapshot};
use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Cancellation is cooperative; dropping evaluation also drops the backend future.
#[derive(Clone, Debug, Default)]
pub struct DecisionCancellation(CancellationToken);
impl DecisionCancellation {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn cancel(&self) {
    self.0.cancel();
  }
  pub fn is_cancelled(&self) -> bool {
    self.0.is_cancelled()
  }
  pub async fn cancelled(&self) {
    self.0.cancelled().await;
  }
}

#[derive(Clone, Debug)]
pub struct BackendDescriptor {
  pub name: String,
  pub version: String,
  pub index_revision: Option<String>,
  /// A false value excludes this backend from local-only requests, including text disclosure.
  pub local: bool,
}
#[derive(Clone, Debug)]
pub struct BackendDecision {
  pub output: DecisionOutput,
  /// Similarity or confidence is never an authorization grant.
  pub confidence: f64,
  pub reason_codes: Vec<String>,
}
#[derive(Clone, Debug, Error)]
pub enum BackendError {
  #[error("backend unavailable")]
  Unavailable,
  #[error("backend rejected input")]
  InvalidInput,
  #[error("backend cancelled")]
  Cancelled,
}

#[async_trait]
pub trait DecisionBackend: Send + Sync {
  fn descriptor(&self) -> BackendDescriptor;
  fn supports(&self, operation: &crate::Operation) -> bool;
  async fn evaluate(
    &self,
    request: &DecisionRequest,
    policy: &PolicySnapshot,
    cancellation: &DecisionCancellation,
  ) -> Result<BackendDecision, BackendError>;
}
