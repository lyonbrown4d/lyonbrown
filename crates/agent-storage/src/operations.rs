mod summary;
use summary::validate_summary;

use crate::{Store, event_tx, schema::runs, verify_epoch};
use agent_core::now_ms;
use anyhow::{Context, Result, ensure};
use diesel::{connection::SimpleConnection, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

diesel::table! {
  operations (action_id) {
    action_id -> Text, run_id -> Text, owner_epoch -> BigInt,
    status -> Text, record -> Text,
  }
}

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

pub(crate) fn initialize(connection: &mut SqliteConnection) -> Result<()> {
  connection.batch_execute(include_str!("operations.sql"))?;
  Ok(())
}

impl Store {
  /// Commit before dispatch. Even an identical identifier is always rejected.
  /// Summaries contain bounded operational metadata, never command input or output.
  pub fn begin_operation(
    &self,
    run: &str,
    epoch: u64,
    action: &str,
    kind: &str,
    summary: Value,
  ) -> Result<()> {
    validate_intent(action, kind, &summary)?;
    self.lock()?.immediate_transaction(|connection| {
      begin_operation_tx(connection, run, epoch, action, kind, summary)
    })
  }

  /// Cancellation after dispatch does not discard the actual operation receipt.
  pub fn finish_operation(
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
    self.lock()?.immediate_transaction(|connection| {
      let mut record = load_operation(connection, action)?.context("operation intent missing")?;
      ensure!(
        record.run_id == run && record.owner_epoch == epoch,
        "operation owner binding mismatch"
      );
      let session: String = runs::table
        .find(run)
        .select(runs::session_id)
        .first(connection)?;
      verify_epoch(connection, &session, epoch)?;
      record.status = status;
      record.result_summary = Some(summary);
      record.finished_at_ms = Some(now_ms());
      write_receipt(connection, &record)?;
      Ok(())
    })
  }

  pub fn operation(&self, action: &str) -> Result<Option<OperationRecord>> {
    load_operation(&mut *self.lock()?, action)
  }

  /// Restart never replays an intent whose dispatch outcome was not committed.
  pub fn recover_operations(&self) -> Result<usize> {
    self.lock()?.immediate_transaction(|connection| {
      let interrupted: Vec<String> = operations::table
        .filter(operations::status.eq("intent"))
        .select(operations::record)
        .load(connection)?;
      for encoded in &interrupted {
        let mut record: OperationRecord = serde_json::from_str(encoded)?;
        record.status = OperationStatus::Uncertain;
        record.result_summary = Some(json!({"reason":"daemon_restarted"}));
        record.finished_at_ms = Some(now_ms());
        write_receipt(connection, &record)?;
      }
      Ok(interrupted.len())
    })
  }
}

fn load_operation(
  connection: &mut SqliteConnection,
  action: &str,
) -> Result<Option<OperationRecord>> {
  let encoded: Option<String> = operations::table
    .find(action)
    .select(operations::record)
    .first(connection)
    .optional()?;
  encoded
    .map(|value| Ok(serde_json::from_str(&value)?))
    .transpose()
}

fn write_receipt(connection: &mut SqliteConnection, record: &OperationRecord) -> Result<()> {
  ensure!(
    diesel::update(
      operations::table
        .find(&record.action_id)
        .filter(operations::status.eq("intent"))
    )
    .set((
      operations::status.eq(record.status.as_str()),
      operations::record.eq(serde_json::to_string(record)?),
    ))
    .execute(connection)?
      == 1,
    "operation outcome already recorded"
  );
  event_tx(
    connection,
    &record.run_id,
    "operation_finished",
    json!({"action_id":record.action_id,"status":record.status,"summary":record.result_summary}),
  )?;
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

pub(crate) fn begin_operation_tx(
  connection: &mut SqliteConnection,
  run: &str,
  epoch: u64,
  action: &str,
  kind: &str,
  summary: Value,
) -> Result<()> {
  let (session, status): (String, String) = runs::table
    .find(run)
    .select((runs::session_id, runs::status))
    .first(connection)?;
  verify_epoch(connection, &session, epoch)?;
  ensure!(
    status == "running",
    "operation requires a running, uncancelled run"
  );
  let prior = operations::table
    .find(action)
    .select(operations::action_id)
    .first::<String>(connection)
    .optional()?;
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
  diesel::insert_into(operations::table)
    .values((
      operations::action_id.eq(action),
      operations::run_id.eq(run),
      operations::owner_epoch.eq(i64::try_from(epoch)?),
      operations::status.eq("intent"),
      operations::record.eq(serde_json::to_string(&record)?),
    ))
    .execute(connection)?;
  event_tx(
    connection,
    run,
    "operation_intent",
    json!({"action_id":action,"kind":kind}),
  )?;
  Ok(())
}
