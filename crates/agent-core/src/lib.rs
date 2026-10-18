//! Application-owned domain records; independent of UI and agent-engine types.
mod records;
pub use records::*;
use std::time::{SystemTime, UNIX_EPOCH};
pub fn new_id() -> String {
  uuid::Uuid::now_v7().to_string()
}
pub fn now_ms() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
    .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
  Accepted,
  Running,
  AwaitingApproval,
  CancelRequested,
  Completed,
  Failed,
  Cancelled,
  Uncertain,
}
impl RunStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Accepted => "accepted",
      Self::Running => "running",
      Self::AwaitingApproval => "awaiting_approval",
      Self::CancelRequested => "cancel_requested",
      Self::Completed => "completed",
      Self::Failed => "failed",
      Self::Cancelled => "cancelled",
      Self::Uncertain => "uncertain",
    }
  }
  pub fn terminal(self) -> bool {
    matches!(
      self,
      Self::Completed | Self::Failed | Self::Cancelled | Self::Uncertain
    )
  }
}
