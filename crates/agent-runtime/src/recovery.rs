use crate::{Result, SessionPersistence, persistence::DurableSession};
use std::sync::Arc;
/// Read-only evidence. Inspection never constructs a model or issues IO.
#[derive(Clone, Debug, Default)]
pub struct RecoveryReport {
  pub pending_tools: Vec<String>,
  pub uncertainty: Option<String>,
}
pub async fn inspect_recovery(
  session_id: &str,
  owner_epoch: u64,
  persistence: Arc<dyn SessionPersistence>,
) -> Result<RecoveryReport> {
  let storage = DurableSession::open(session_id, owner_epoch, persistence).await?;
  let state = storage.state.lock().await;
  let pending_tools = state
    .pending_tools
    .values()
    .map(|tool| format!("{} ({})", tool.name, tool.call_id))
    .collect();
  let uncertainty = crate::persistence::ensure_snapshot_certain(&state)
    .err()
    .map(|e| e.to_string());
  Ok(RecoveryReport {
    pending_tools,
    uncertainty,
  })
}
