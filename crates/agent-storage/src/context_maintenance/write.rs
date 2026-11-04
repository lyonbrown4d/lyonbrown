use super::{ensure_idle, entity, save};
use crate::{Store, schema::runs, verify_epoch};
use agent_core::context_maintenance::{
  ContextMaintenanceJob as Job, ContextMaintenanceKind as Kind, ContextMaintenanceState as State,
};
use anyhow::{Context, Result, ensure};
use sea_orm::{Set, prelude::*};
impl Store {
  pub async fn begin_context_maintenance(
    &self,
    session_id: &str,
    run_id: &str,
    epoch: u64,
    expected_revision: u64,
    kind: Kind,
  ) -> Result<Job> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      verify_epoch(&transaction, session_id, epoch).await?;
      ensure_idle(&transaction, session_id).await?;
      let outstanding = entity::Entity::find()
        .filter(entity::Column::State.is_in([
          State::Preparing.code(),
          State::Sending.code(),
          State::Uncertain.code(),
        ]))
        .count(&transaction)
        .await?;
      ensure!(outstanding < 128, "context maintenance capacity reached");
      crate::input_fence::ensure_no_input_cancellation(&transaction, session_id).await?;
      crate::changes::ensure_no_prepared_change(&transaction, session_id).await?;
      let active = runs::Entity::find()
        .filter(runs::Column::SessionId.eq(session_id))
        .filter(runs::Column::Status.is_in([
          "accepted",
          "running",
          "awaiting_approval",
          "cancel_requested",
          "uncertain",
        ]))
        .count(&transaction)
        .await?;
      ensure!(
        active == 0,
        "finish or reconcile the session run before changing context"
      );
      let run = runs::Entity::find_by_id(run_id)
        .one(&transaction)
        .await?
        .context("context audit run missing")?;
      ensure!(
        run.session_id == session_id,
        "context audit run belongs to another session"
      );
      let now = agent_core::now_ms();
      let job = Job {
        id: agent_core::new_id(),
        session_id: session_id.into(),
        run_id: run_id.into(),
        owner_epoch: epoch,
        expected_revision,
        kind,
        state: State::Preparing,
        cancel_requested: false,
        abandon_request_id: None,
        abandon_reason: None,
        created_at_ms: now,
        updated_at_ms: now,
      };
      entity::ActiveModel {
        id: Set(job.id.clone()),
        session_id: Set(session_id.into()),
        state: Set(job.state.code()),
        record: Set(serde_json::to_string(&job)?),
      }
      .insert(&transaction)
      .await?;
      crate::event_tx(
        &transaction,
        run_id,
        "context_maintenance_started",
        serde_json::to_value(&job)?,
      )
      .await?;
      Ok(job)
    }
    .await;
    match result {
      Ok(job) => {
        transaction.commit().await?;
        Ok(job)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
  pub async fn finish_context_maintenance(
    &self,
    id: &str,
    epoch: u64,
    outcome: State,
  ) -> Result<Job> {
    ensure!(
      matches!(
        outcome,
        State::Completed | State::Failed | State::Cancelled | State::Uncertain
      ),
      "terminal context outcome required; abandonment needs explicit resolution"
    );
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let row = entity::Entity::find_by_id(id)
        .one(&transaction)
        .await?
        .context("context job missing")?;
      let mut job: Job = serde_json::from_str(&row.record)?;
      verify_epoch(&transaction, &job.session_id, epoch).await?;
      ensure!(
        job.owner_epoch == epoch,
        "context maintenance epoch changed"
      );
      if !matches!(job.state, State::Preparing | State::Sending) {
        return Ok(job);
      }
      ensure!(
        !(job.state == State::Sending && outcome == State::Cancelled),
        "sent context outcome cannot be assumed cancelled"
      );
      job.state = outcome;
      job.updated_at_ms = agent_core::now_ms();
      save(&transaction, &job).await?;
      Ok(job)
    }
    .await;
    match result {
      Ok(job) => {
        transaction.commit().await?;
        Ok(job)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
  pub async fn request_context_cancellation(&self, id: &str, epoch: u64) -> Result<Job> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let row = entity::Entity::find_by_id(id)
        .one(&transaction)
        .await?
        .context("context job missing")?;
      let mut job: Job = serde_json::from_str(&row.record)?;
      verify_epoch(&transaction, &job.session_id, epoch).await?;
      ensure!(
        job.owner_epoch == epoch,
        "context maintenance epoch changed"
      );
      if matches!(job.state, State::Preparing | State::Sending) {
        job.cancel_requested = true;
        job.updated_at_ms = agent_core::now_ms();
        save(&transaction, &job).await?;
      }
      Ok(job)
    }
    .await;
    match result {
      Ok(job) => {
        transaction.commit().await?;
        Ok(job)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}
