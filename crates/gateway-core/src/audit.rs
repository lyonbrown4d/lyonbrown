use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, utoipa::ToSchema)]
pub struct TokenUsage {
  pub input_tokens: u64,
  pub output_tokens: u64,
  pub cached_input_tokens: u64,
  pub reasoning_tokens: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditStatus {
  Pending,
  Succeeded,
  Failed,
  Cancelled,
  Uncertain,
}

/// Request metadata only. Pending intent must be durable before the upstream side effect.
#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AuditRecord {
  pub request_id: String,
  pub model: String,
  #[serde(default)]
  pub principal: Option<crate::AccessPrincipal>,
  #[serde(default)]
  pub reserved_tokens: u64,
  #[serde(default)]
  pub usage: Option<TokenUsage>,
  pub status: AuditStatus,
  pub elapsed_ms: u64,
  pub policy_revision: String,
}
#[derive(Debug, thiserror::Error)]
#[error("audit persistence unavailable")]
pub struct AuditError;
#[derive(Debug, thiserror::Error)]
pub enum AdmissionError {
  #[error("API key token budget exhausted")]
  BudgetExceeded,
  #[error("audit persistence unavailable")]
  Unavailable,
}
#[async_trait]
pub trait AuditSink: Send + Sync {
  async fn record(&self, record: AuditRecord) -> Result<(), AuditError>;
  async fn admit(&self, record: AuditRecord) -> Result<(), AdmissionError> {
    self
      .record(record)
      .await
      .map_err(|_| AdmissionError::Unavailable)
  }
}
