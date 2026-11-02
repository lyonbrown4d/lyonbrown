use crate::schema::operations;
use sea_orm::{QuerySelect, Set, prelude::*};
mod summary;
use summary::validate_summary;

use crate::{Store, event_tx, schema::runs, verify_epoch};
use agent_core::now_ms;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A dispatched operation has one final receipt. Unknown outcomes cannot be replayed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
  Intent,
  Completed,
  Failed,
  Uncertain,
}
impl OperationStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Intent => "intent",
      Self::Completed => "completed",
      Self::Failed => "failed",
      Self::Uncertain => "uncertain",
    }
  }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OperationRecord {
  pub action_id: String,
  pub run_id: String,
  pub owner_epoch: u64,
  pub kind: String,
  pub status: OperationStatus,
  pub intent_summary: Value,
  pub result_summary: Option<Value>,
  pub created_at_ms: i64,
  pub finished_at_ms: Option<i64>,
}

impl Store {
  /// Commit before dispatch. Even an identical identifier is always rejected.
  /// Summaries contain bounded operational metadata, never command input or output.
  pub async fn begin_operation(
    &self,
    run: &str,
    epoch: u64,
    action: &str,
    kind: &str,
    summary: Value,
  ) -> Result<()> {
    validate_intent(action, kind, &summary)?;
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> =
        async { begin_operation_tx(connection, run, epoch, action, kind, summary).await }.await;
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

  /// Cancellation after dispatch does not discard the actual operation receipt.
  pub async fn finish_operation(
    &self,
    run: &str,
    epoch: u64,
    action: &str,
    status: OperationStatus,
    summary: Value,
  ) -> Result<()> {
    ensure!(
      status != OperationStatus::Intent,
      "operation receipt must be terminal"
    );
    validate_summary(&summary)?;
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let mut record = load_operation(connection, action)
          .await?
          .context("operation intent missing")?;
        ensure!(
          record.run_id == run && record.owner_epoch == epoch,
          "operation owner binding mismatch"
        );
        let session: String = runs::Entity::find()
          .filter(runs::Column::Id.eq(run))
          .select_only()
          .column(runs::Column::SessionId)
          .into_tuple::<String>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
        verify_epoch(connection, &session, epoch).await?;
        record.status = status;
        record.result_summary = Some(summary);
        record.finished_at_ms = Some(now_ms());
        write_receipt(connection, &record).await?;
        Ok(())
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

  pub async fn operation(&self, action: &str) -> Result<Option<OperationRecord>> {
    load_operation(self.connection(), action).await
  }

  /// Restart never replays an intent whose dispatch outcome was not committed.
  pub async fn recover_operations(&self) -> Result<usize> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let interrupted: Vec<String> = operations::Entity::find()
          .filter(operations::Column::Status.eq("intent"))
          .select_only()
          .column(operations::Column::Record)
          .into_tuple::<String>()
          .all(connection)
          .await?;
        for encoded in &interrupted {
          let mut record: OperationRecord = serde_json::from_str(encoded)?;
          record.status = OperationStatus::Uncertain;
          record.result_summary = Some(json!({"reason":"daemon_restarted"}));
          record.finished_at_ms = Some(now_ms());
          write_receipt(connection, &record).await?;
        }
        Ok(interrupted.len())
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
}

async fn load_operation(
  connection: &impl ConnectionTrait,
  action: &str,
) -> Result<Option<OperationRecord>> {
  let encoded: Option<String> = operations::Entity::find()
    .filter(operations::Column::ActionId.eq(action))
    .select_only()
    .column(operations::Column::Record)
    .into_tuple::<String>()
    .one(connection)
    .await?;
  encoded
    .map(|value| Ok(serde_json::from_str(&value)?))
    .transpose()
}

async fn write_receipt(connection: &impl ConnectionTrait, record: &OperationRecord) -> Result<()> {
  ensure!(
    operations::Entity::update_many()
      .filter(operations::Column::ActionId.eq(&record.action_id))
      .filter(operations::Column::Status.eq("intent"))
      .col_expr(
        operations::Column::Status,
        Expr::value((record.status.as_str()).to_owned())
      )
      .col_expr(
        operations::Column::Record,
        Expr::value((serde_json::to_string(record)?).to_owned())
      )
      .exec(connection)
      .await
      .map(|result| result.rows_affected)?
      == 1,
    "operation outcome already recorded"
  );
  event_tx(
    connection,
    &record.run_id,
    "operation_finished",
    json!({"action_id":record.action_id,"status":record.status,"summary":record.result_summary}),
  )
  .await?;
  Ok(())
}

pub(crate) fn validate_intent(action: &str, kind: &str, summary: &Value) -> Result<()> {
  ensure!(
    !action.is_empty() && action.len() <= 128,
    "invalid operation identifier"
  );
  ensure!(
    !kind.is_empty()
      && kind.len() <= 64
      && kind
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
    "invalid operation kind"
  );
  validate_summary(summary)
}

pub(crate) async fn begin_operation_tx(
  connection: &impl ConnectionTrait,
  run: &str,
  epoch: u64,
  action: &str,
  kind: &str,
  summary: Value,
) -> Result<()> {
  let (session, status): (String, String) = runs::Entity::find()
    .filter(runs::Column::Id.eq(run))
    .select_only()
    .column(runs::Column::SessionId)
    .column(runs::Column::Status)
    .into_tuple::<(String, String)>()
    .one(connection)
    .await?
    .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
  verify_epoch(connection, &session, epoch).await?;
  ensure!(
    status == "running",
    "operation requires a running, uncancelled run"
  );
  let prior = operations::Entity::find()
    .filter(operations::Column::ActionId.eq(action))
    .select_only()
    .column(operations::Column::ActionId)
    .into_tuple::<String>()
    .one(connection)
    .await?;
  ensure!(
    prior.is_none(),
    "operation identifier already recorded; replay is forbidden"
  );
  let record = OperationRecord {
    action_id: action.into(),
    run_id: run.into(),
    owner_epoch: epoch,
    kind: kind.into(),
    status: OperationStatus::Intent,
    intent_summary: summary,
    result_summary: None,
    created_at_ms: now_ms(),
    finished_at_ms: None,
  };
  operations::ActiveModel {
    action_id: Set((action).to_owned()),
    run_id: Set((run).to_owned()),
    owner_epoch: Set((i64::try_from(epoch)?).to_owned()),
    status: Set(("intent").to_owned()),
    record: Set((serde_json::to_string(&record)?).to_owned()),
  }
  .insert(connection)
  .await?;
  event_tx(
    connection,
    run,
    "operation_intent",
    json!({"action_id":action,"kind":kind}),
  )
  .await?;
  Ok(())
}
