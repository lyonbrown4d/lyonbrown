//! Metadata for host-owned context maintenance; never contains summary text or credentials.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ContextMaintenanceState {
  Preparing,
  Sending,
  Completed,
  Failed,
  Cancelled,
  Uncertain,
  Abandoned,
}
impl ContextMaintenanceState {
  pub fn code(self) -> i16 {
    match self {
      Self::Preparing => 0,
      Self::Sending => 1,
      Self::Completed => 2,
      Self::Failed => 3,
      Self::Cancelled => 4,
      Self::Uncertain => 5,
      Self::Abandoned => 6,
    }
  }
  pub fn blocks_admission(self) -> bool {
    matches!(
      self,
      Self::Preparing | Self::Sending | Self::Uncertain | Self::Abandoned
    )
  }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMaintenanceKind {
  Compact,
  Undo,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextMaintenanceJob {
  pub id: String,
  pub session_id: String,
  pub run_id: String,
  pub owner_epoch: u64,
  pub expected_revision: u64,
  pub kind: ContextMaintenanceKind,
  pub state: ContextMaintenanceState,
  pub cancel_requested: bool,
  pub abandon_request_id: Option<String>,
  pub abandon_reason: Option<String>,
  pub created_at_ms: i64,
  pub updated_at_ms: i64,
}
