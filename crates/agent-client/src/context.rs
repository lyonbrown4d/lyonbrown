use crate::AgentClient;
use agent_protocol::v1;
use anyhow::Result;
impl AgentClient {
  pub async fn context_maintenance(&self, session_id: &str) -> Result<v1::ContextMaintenanceJob> {
    Ok(
      self
        .inner
        .clone()
        .get_context_maintenance(v1::SessionRequest {
          session_id: session_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn abandon_context(
    &self,
    job_id: &str,
    owner_epoch: u64,
    request_id: &str,
    reason: &str,
  ) -> Result<v1::AbandonContextResult> {
    Ok(
      self
        .inner
        .clone()
        .abandon_context(v1::AbandonContextRequest {
          job_id: job_id.into(),
          owner_epoch,
          request_id: request_id.into(),
          reason: reason.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn cancel_context(
    &self,
    job_id: &str,
    owner_epoch: u64,
  ) -> Result<v1::ContextMaintenanceJob> {
    Ok(
      self
        .inner
        .clone()
        .cancel_context(v1::CancelContextRequest {
          job_id: job_id.into(),
          owner_epoch,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn context_status(&self, session_id: &str) -> Result<v1::ContextStatus> {
    Ok(
      self
        .inner
        .clone()
        .get_context_status(v1::SessionRequest {
          session_id: session_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn context_summary(&self, session_id: &str) -> Result<v1::ContextSummary> {
    Ok(
      self
        .inner
        .clone()
        .get_context_summary(v1::SessionRequest {
          session_id: session_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn compact_context(
    &self,
    session_id: &str,
    owner_epoch: u64,
    expected_revision: u64,
    focus: &str,
  ) -> Result<v1::ContextStatus> {
    Ok(
      self
        .inner
        .clone()
        .compact_context(v1::CompactContextRequest {
          session_id: session_id.into(),
          owner_epoch,
          expected_revision,
          focus: focus.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn undo_context_compaction(
    &self,
    session_id: &str,
    owner_epoch: u64,
    expected_revision: u64,
  ) -> Result<v1::ContextStatus> {
    Ok(
      self
        .inner
        .clone()
        .undo_context_compaction(v1::UndoContextCompactionRequest {
          session_id: session_id.into(),
          owner_epoch,
          expected_revision,
        })
        .await?
        .into_inner(),
    )
  }
}
