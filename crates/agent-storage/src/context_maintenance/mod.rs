//! One durable maintenance admission fence per Session, sharing snapshot/event transactions.
mod abandon;
mod entity;
mod observer;
mod write;
use crate::Store;
use agent_core::context_maintenance::{ContextMaintenanceJob, ContextMaintenanceState as State};
use anyhow::{Result, ensure};
pub(crate) use observer::observe_snapshot;
use sea_orm::{QueryOrder, QuerySelect, prelude::*};
pub(crate) async fn ensure_idle(connection: &impl ConnectionTrait, session: &str) -> Result<()> {
  let count = entity::Entity::find()
    .filter(entity::Column::SessionId.eq(session))
    .filter(entity::Column::State.is_in([
      State::Preparing.code(),
      State::Sending.code(),
      State::Uncertain.code(),
      State::Abandoned.code(),
    ]))
    .count(connection)
    .await?;
  ensure!(
    count == 0,
    "session has active, uncertain or abandoned context maintenance; inspect its job or create a new session"
  );
  Ok(())
}
pub(super) async fn save(
  connection: &impl ConnectionTrait,
  job: &ContextMaintenanceJob,
) -> Result<()> {
  let changed = entity::Entity::update_many()
    .filter(entity::Column::Id.eq(&job.id))
    .col_expr(entity::Column::State, Expr::value(job.state.code()))
    .col_expr(
      entity::Column::Record,
      Expr::value(serde_json::to_string(job)?),
    )
    .exec(connection)
    .await?;
  ensure!(
    changed.rows_affected == 1,
    "context maintenance job missing"
  );
  crate::event_tx(
    connection,
    &job.run_id,
    "context_maintenance_updated",
    serde_json::to_value(job)?,
  )
  .await?;
  Ok(())
}
impl Store {
  pub async fn context_maintenance_job(&self, id: &str) -> Result<Option<ContextMaintenanceJob>> {
    entity::Entity::find_by_id(id)
      .one(self.connection())
      .await?
      .map(|row| Ok(serde_json::from_str(&row.record)?))
      .transpose()
  }
  pub async fn latest_context_maintenance(
    &self,
    session: &str,
  ) -> Result<Option<ContextMaintenanceJob>> {
    entity::Entity::find()
      .filter(entity::Column::SessionId.eq(session))
      .order_by_desc(entity::Column::Id)
      .one(self.connection())
      .await?
      .map(|row| Ok(serde_json::from_str(&row.record)?))
      .transpose()
  }
  pub async fn context_maintenance_count(&self) -> Result<usize> {
    Ok(usize::try_from(
      entity::Entity::find()
        .filter(entity::Column::State.is_in([
          State::Preparing.code(),
          State::Sending.code(),
          State::Uncertain.code(),
        ]))
        .count(self.connection())
        .await?,
    )?)
  }
  /// Recovery never sends model IO. Preparing jobs are returned for pure unsent-snapshot cleanup.
  pub async fn recover_context_maintenance(&self) -> Result<Vec<ContextMaintenanceJob>> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let rows = entity::Entity::find()
        .filter(entity::Column::State.is_in([State::Preparing.code(), State::Sending.code()]))
        .order_by_asc(entity::Column::Id)
        .limit(1024)
        .all(&transaction)
        .await?;
      let mut preparing = Vec::new();
      for row in rows {
        let mut job: ContextMaintenanceJob = serde_json::from_str(&row.record)?;
        if job.state == State::Sending {
          job.state = State::Uncertain;
          job.updated_at_ms = agent_core::now_ms();
          save(&transaction, &job).await?;
        } else {
          preparing.push(job);
        }
      }
      Ok(preparing)
    }
    .await;
    match result {
      Ok(jobs) => {
        transaction.commit().await?;
        Ok(jobs)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}
