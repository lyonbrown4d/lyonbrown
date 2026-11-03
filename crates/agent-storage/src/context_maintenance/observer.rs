use super::{entity, save};
use agent_core::context_maintenance::{
  ContextMaintenanceJob as Job, ContextMaintenanceState as State,
};
use anyhow::{Result, ensure};
use sea_orm::prelude::*;
/// Called inside the same transaction as the Runtime snapshot and semantic events.
pub(crate) async fn observe_snapshot(
  db: &impl ConnectionTrait,
  session: &str,
  epoch: u64,
  kind: &str,
) -> Result<()> {
  let target = match kind {
    "context_summary_sending" => State::Sending,
    "context_compaction_committed" | "context_compaction_undone" => State::Completed,
    "context_summary_failed" => State::Failed,
    "context_summary_cancelled_before_send" => State::Cancelled,
    "execution_uncertain" => State::Uncertain,
    _ => return Ok(()),
  };
  let row = entity::Entity::find()
    .filter(entity::Column::SessionId.eq(session))
    .filter(entity::Column::State.is_in([State::Preparing.code(), State::Sending.code()]))
    .one(db)
    .await?;
  let Some(row) = row else {
    return Ok(());
  };
  let mut job: Job = serde_json::from_str(&row.record)?;
  ensure!(
    job.owner_epoch == epoch,
    "context job snapshot writer fenced"
  );
  if target == State::Sending {
    ensure!(
      job.state == State::Preparing && !job.cancel_requested,
      "context request was cancelled before sending"
    );
  }
  ensure!(
    !(job.state == State::Sending && target == State::Cancelled),
    "sent summary cannot be discarded as unsent"
  );
  let target = if target == State::Failed && job.cancel_requested {
    if job.state == State::Preparing {
      State::Cancelled
    } else {
      State::Uncertain
    }
  } else {
    target
  };
  job.state = target;
  job.updated_at_ms = agent_core::now_ms();
  save(db, &job).await
}
