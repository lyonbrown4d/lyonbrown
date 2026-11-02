//! Node-local durable state. Critical writes include an event and outbox row.
#![forbid(unsafe_code)]
use sea_orm::sea_query::ExprTrait;
mod approval_consumption;
mod approvals;
mod approved_browser_action;
mod approved_operation;
mod browser_actions;
mod connection;
mod control;
mod error;
pub use error::{is_database_error, public_error_message, safe_storage_error};
mod dispatch;
mod input_fence;
mod memory;
mod migration;
mod model_bindings;
mod operations;
mod peers;
mod provider_profiles;
mod recovery;
mod runs;
mod schema;
mod sessions;
mod snapshots;
mod task_acceptance;
mod tasks;
use agent_core::{Event, now_ms};
use anyhow::{Context, Result};
pub use approved_browser_action::{ApprovedBrowserAction, BrowserActionAuthorization};
pub use approved_operation::{ApprovedOperation, OperationAuthorization};
pub use model_bindings::ModelBinding;
pub use operations::{OperationRecord, OperationStatus};
pub use provider_profiles::ProviderProfileError;
pub use recovery::{RecoverRun, RecoveryAction, RunRecovery};
use schema::{events, outbox, runs as run_rows, sessions};
use sea_orm::{
  DatabaseConnection, DatabaseTransaction, QueryOrder, QuerySelect, Set, TransactionTrait,
  prelude::*, sea_query::Expr,
};

#[derive(Clone)]
pub struct Store {
  connection: DatabaseConnection,
}
impl Store {
  /// Shared native async connection for repositories participating in this store.
  pub fn connection(&self) -> &DatabaseConnection {
    &self.connection
  }
  async fn begin_write(&self) -> Result<DatabaseTransaction> {
    let transaction = self.connection.begin().await?;
    let locked = schema::write_lock::Entity::update_many()
      .col_expr(
        schema::write_lock::Column::Revision,
        Expr::col(schema::write_lock::Column::Revision).add(1),
      )
      .filter(schema::write_lock::Column::Id.eq(1))
      .exec(&transaction)
      .await?;
    anyhow::ensure!(locked.rows_affected == 1, "database write fence missing");
    Ok(transaction)
  }
  pub async fn events(&self, run_id: &str, after: u64, limit: usize) -> Result<Vec<Event>> {
    let rows = events::Entity::find()
      .filter(events::Column::RunId.eq(run_id))
      .filter(events::Column::Cursor.gt(i64::try_from(after)?))
      .order_by_asc(events::Column::Cursor)
      .limit(limit.min(1000) as u64)
      .select_only()
      .column(events::Column::Record)
      .into_tuple::<String>()
      .all(self.connection())
      .await?;
    decode(rows)
  }
  pub async fn append_event(
    &self,
    run_id: &str,
    kind: &str,
    payload: serde_json::Value,
  ) -> Result<Event> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> =
        async { event_tx(connection, run_id, kind, payload).await }.await;
      match result {
        Ok(value) => {
          transaction.commit().await?;
          Ok::<_, anyhow::Error>(value)
        }
        Err(error) => {
          transaction.rollback().await?;
          Err(error)
        }
      }
    }
  }
  pub async fn append_event_owned(
    &self,
    run_id: &str,
    epoch: u64,
    kind: &str,
    payload: serde_json::Value,
  ) -> Result<Event> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let run = run_rows::Entity::find_by_id(run_id)
          .one(connection)
          .await?
          .context("run missing")?;
        verify_epoch(connection, &run.session_id, epoch).await?;
        event_tx(connection, run_id, kind, payload).await
      }
      .await;
      match result {
        Ok(value) => {
          transaction.commit().await?;
          Ok::<_, anyhow::Error>(value)
        }
        Err(error) => {
          transaction.rollback().await?;
          Err(error)
        }
      }
    }
  }
  pub async fn pending_outbox(&self, limit: usize) -> Result<Vec<Event>> {
    let pending = outbox::Entity::find()
      .filter(outbox::Column::Delivered.eq(0))
      .order_by_asc(outbox::Column::RunId)
      .order_by_asc(outbox::Column::Cursor)
      .limit(limit.min(1000) as u64)
      .all(self.connection())
      .await?;
    let mut result = Vec::with_capacity(pending.len());
    for row in pending {
      let event = events::Entity::find_by_id((row.run_id, row.cursor))
        .one(self.connection())
        .await?
        .context("outbox event missing")?;
      result.push(serde_json::from_str(&event.record)?);
    }
    Ok(result)
  }
  pub async fn acknowledge_outbox(&self, run_id: &str, cursor: u64) -> Result<()> {
    outbox::Entity::update_many()
      .filter(outbox::Column::RunId.eq(run_id))
      .filter(outbox::Column::Cursor.eq(i64::try_from(cursor)?))
      .col_expr(outbox::Column::Delivered, Expr::value(1))
      .exec(self.connection())
      .await?;
    Ok(())
  }
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(records: Vec<String>) -> Result<Vec<T>> {
  records
    .into_iter()
    .map(|record| Ok(serde_json::from_str(&record)?))
    .collect()
}
pub(crate) async fn verify_epoch(
  connection: &impl ConnectionTrait,
  session_id: &str,
  expected: u64,
) -> Result<()> {
  let row = sessions::Entity::find_by_id(session_id)
    .one(connection)
    .await?
    .context("session missing")?;
  anyhow::ensure!(
    u64::try_from(row.owner_epoch)? == expected,
    "stale session owner epoch"
  );
  Ok(())
}
pub(crate) async fn event_tx(
  connection: &impl ConnectionTrait,
  run_id: &str,
  kind: &str,
  payload: serde_json::Value,
) -> Result<Event> {
  let previous = events::Entity::find()
    .filter(events::Column::RunId.eq(run_id))
    .order_by_desc(events::Column::Cursor)
    .one(connection)
    .await?
    .map(|row| row.cursor)
    .unwrap_or(0);
  let cursor = previous.checked_add(1).context("event cursor exhausted")?;
  let event = Event {
    cursor: u64::try_from(cursor)?,
    run_id: run_id.into(),
    kind: kind.into(),
    payload_json: serde_json::to_string(&payload)?,
    created_at_ms: now_ms(),
  };
  events::ActiveModel {
    run_id: Set(run_id.into()),
    cursor: Set(cursor),
    record: Set(serde_json::to_string(&event)?),
  }
  .insert(connection)
  .await?;
  outbox::ActiveModel {
    run_id: Set(run_id.into()),
    cursor: Set(cursor),
    delivered: Set(0),
  }
  .insert(connection)
  .await?;
  Ok(event)
}
