use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
  pub status: AuditStatus,
  pub elapsed_ms: u64,
  pub policy_revision: String,
}
#[derive(Debug, thiserror::Error)]
#[error("audit persistence unavailable")]
pub struct AuditError;
#[async_trait]
pub trait AuditSink: Send + Sync {
  async fn record(&self, record: AuditRecord) -> Result<(), AuditError>;
}
