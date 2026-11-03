use super::{entity, save};
use crate::{
  Store,
  schema::{runs, sessions},
  verify_epoch,
};
use agent_core::{
  Session,
  context_maintenance::{ContextMaintenanceJob as Job, ContextMaintenanceState as State},
};
use anyhow::{Context, Result, ensure};
use sea_orm::prelude::*;
impl Store {
  /// Explicit human resolution fences the old writer without clearing or replaying its snapshot.
  pub async fn abandon_context_maintenance(
    &self,
    id: &str,
    epoch: u64,
    request_id: &str,
    reason: &str,
  ) -> Result<(Job, Session)> {
    ensure!(
      !uuid::Uuid::parse_str(request_id)?.is_nil(),
      "resolution request UUID required"
    );
    ensure!(
      !reason.trim().is_empty() && reason.len() <= 2048 && !reason.chars().any(char::is_control),
      "resolution explanation required (1..2048 bytes)"
    );
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let row = entity::Entity::find_by_id(id).one(&transaction).await?.context("context job missing")?;
      let mut job: Job = serde_json::from_str(&row.record)?;
      let row = sessions::Entity::find_by_id(&job.session_id).one(&transaction).await?.context("session missing")?;
      let mut session: Session = serde_json::from_str(&row.record)?;
      if job.state == State::Abandoned {
        ensure!(job.abandon_request_id.as_deref() == Some(request_id)
          && job.abandon_reason.as_deref() == Some(reason) && job.owner_epoch == epoch
          && session.owner_epoch == epoch.checked_add(1).context("owner epoch exhausted")?, "resolution request conflict");
        return Ok((job, session));
      }
      verify_epoch(&transaction, &job.session_id, epoch).await?;
      ensure!(job.owner_epoch == epoch && job.state == State::Uncertain, "only an uncertain context job can be abandoned");
      let active = runs::Entity::find().filter(runs::Column::SessionId.eq(&job.session_id))
        .filter(runs::Column::Status.is_in(["accepted", "running", "awaiting_approval", "cancel_requested", "uncertain"]))
        .count(&transaction).await?;
      ensure!(active == 0, "finish or reconcile the session run before abandoning context");
      session.owner_epoch = epoch.checked_add(1).context("owner epoch exhausted")?;
      let changed = sessions::Entity::update_many().filter(sessions::Column::Id.eq(&job.session_id))
        .filter(sessions::Column::OwnerEpoch.eq(i64::try_from(epoch)?))
        .col_expr(sessions::Column::OwnerEpoch, Expr::value(i64::try_from(session.owner_epoch)?))
        .col_expr(sessions::Column::Record, Expr::value(serde_json::to_string(&session)?)).exec(&transaction).await?;
      ensure!(changed.rows_affected == 1, "owner epoch conflict");
      job.state = State::Abandoned; job.abandon_request_id = Some(request_id.into()); job.abandon_reason = Some(reason.into());
      job.updated_at_ms = agent_core::now_ms(); save(&transaction, &job).await?;
      crate::event_tx(&transaction, &job.run_id, "context_maintenance_abandoned", serde_json::json!({
        "job_id":id,"request_id":request_id,"previous_owner_epoch":epoch,"owner_epoch":session.owner_epoch,
        "reason":reason,"new_session_required":true
      })).await?;
      Ok((job, session))
    }.await;
    match result {
      Ok(result) => {
        transaction.commit().await?;
        Ok(result)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}
