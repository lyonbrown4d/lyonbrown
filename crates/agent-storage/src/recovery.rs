use agent_core::{Run, Session, new_id, now_ms};
use anyhow::{Context, Result, ensure};
use diesel::sql_types::{Nullable, Text};
use diesel::{connection::SimpleConnection, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
  Store, event_tx,
  schema::{runs, sessions, tasks},
  verify_epoch,
};

diesel::define_sql_function! {
  fn json_extract(value: Text, path: Text) -> Nullable<Text>;
}

fn unresolved_tasks(connection: &mut SqliteConnection, session: &str) -> Result<i64> {
  Ok(
    tasks::table
      .filter(json_extract(tasks::record, "$.envelope.session_id").eq(session))
      .filter(json_extract(tasks::record, "$.status").ne_all([
        "completed",
        "failed",
        "cancelled",
        "rejected",
      ]))
      .count()
      .get_result(connection)?,
  )
}

diesel::table! {
  run_recoveries (run_id) {
    run_id -> Text, idempotency_key -> Text, record -> Text,
  }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
  Abandon,
  ContinueNewSession,
}

/// A resolution records a human decision; it never changes the original outcome.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunRecovery {
  pub run_id: String,
  pub idempotency_key: String,
  pub requested_owner_epoch: u64,
  pub fenced_owner_epoch: u64,
  pub action: RecoveryAction,
  pub note: String,
  pub prompt: String,
  pub session_id: Option<String>,
  pub replacement_run_id: Option<String>,
  pub created_at_ms: i64,
}

pub struct RecoverRun<'a> {
  pub run_id: &'a str,
  pub owner_epoch: u64,
  pub node_id: &'a str,
  pub action: RecoveryAction,
  pub idempotency_key: &'a str,
  pub note: &'a str,
  pub prompt: &'a str,
}

pub(crate) fn initialize(connection: &mut SqliteConnection) -> Result<()> {
  connection.batch_execute("CREATE TABLE IF NOT EXISTS run_recoveries(run_id TEXT PRIMARY KEY, idempotency_key TEXT NOT NULL UNIQUE, record TEXT NOT NULL);")?;
  Ok(())
}

impl Store {
  pub fn unresolved_recovery_tasks(&self, session: &str) -> Result<i64> {
    unresolved_tasks(&mut *self.lock()?, session)
  }

  pub fn run_recovery(&self, id: &str) -> Result<Option<RunRecovery>> {
    let record: Option<String> = run_recoveries::table
      .find(id)
      .select(run_recoveries::record)
      .first(&mut *self.lock()?)
      .optional()?;
    record
      .map(|json| Ok(serde_json::from_str(&json)?))
      .transpose()
  }

  /// Persist the explicit decision and any fresh execution intent atomically.
  /// One original run has one resolution. Retries must reproduce every input.
  pub fn recover_run(&self, input: RecoverRun<'_>) -> Result<(RunRecovery, bool)> {
    ensure!(
      !input.idempotency_key.is_empty() && input.idempotency_key.len() <= 256,
      "invalid idempotency key"
    );
    ensure!(
      !input.note.trim().is_empty() && input.note.len() <= 4096,
      "human verification note is required (maximum 4096 bytes)"
    );
    match input.action {
      RecoveryAction::Abandon => {
        ensure!(input.prompt.is_empty(), "abandon must not include a prompt")
      }
      RecoveryAction::ContinueNewSession => ensure!(
        !input.prompt.trim().is_empty() && input.prompt.len() <= 1_048_576,
        "a fresh prompt is required (maximum 1048576 bytes)"
      ),
    }
    self.lock()?.immediate_transaction(|connection| {
      let record: String = runs::table
        .find(input.run_id)
        .select(runs::record)
        .first(connection)?;
      let original: Run = serde_json::from_str(&record)?;
      let record: String = sessions::table
        .find(&original.session_id)
        .select(sessions::record)
        .first(connection)?;
      let mut session: Session = serde_json::from_str(&record)?;
      ensure!(
        session.owner_node_id == input.node_id && !session.id.starts_with("worker:"),
        "only the local Master can resolve this run"
      );
      ensure!(
        original.status == "uncertain",
        "only an uncertain run can be resolved"
      );
      let previous: Option<String> = run_recoveries::table
        .find(input.run_id)
        .select(run_recoveries::record)
        .first(connection)
        .optional()?;
      if let Some(record) = previous {
        let previous: RunRecovery = serde_json::from_str(&record)?;
        verify_epoch(
          connection,
          &original.session_id,
          previous.fenced_owner_epoch,
        )?;
        ensure!(
          previous.requested_owner_epoch == input.owner_epoch
            && previous.idempotency_key == input.idempotency_key
            && previous.action == input.action
            && previous.note == input.note
            && previous.prompt == input.prompt,
          "run already resolved; retry must match the original recovery request"
        );
        return Ok((previous, false));
      }
      verify_epoch(connection, &original.session_id, input.owner_epoch)?;
      ensure!(
        unresolved_tasks(connection, &original.session_id)? == 0,
        "reconcile and cancel or accept/reject unresolved Tasks in the original session before Run recovery"
      );
      session.owner_epoch = input
        .owner_epoch
        .checked_add(1)
        .context("owner epoch exhausted")?;
      let mut resolution = RunRecovery {
        run_id: original.id.clone(),
        idempotency_key: input.idempotency_key.into(),
        requested_owner_epoch: input.owner_epoch,
        fenced_owner_epoch: session.owner_epoch,
        action: input.action,
        note: input.note.into(),
        prompt: input.prompt.into(),
        session_id: None,
        replacement_run_id: None,
        created_at_ms: now_ms(),
      };
      if input.action == RecoveryAction::ContinueNewSession {
        let fresh = Session {
          id: new_id(),
          title: format!("Recovery of {}", original.id),
          workspace: session.workspace.clone(),
          owner_node_id: input.node_id.into(),
          owner_epoch: 1,
          created_at_ms: now_ms(),
        };
        let run = Run {
          id: new_id(),
          session_id: fresh.id.clone(),
          status: "accepted".into(),
          prompt: input.prompt.into(),
          result: String::new(),
          error: String::new(),
          created_at_ms: now_ms(),
        };
        diesel::insert_into(sessions::table)
          .values((
            sessions::id.eq(&fresh.id),
            sessions::owner_epoch.eq(1_i64),
            sessions::record.eq(serde_json::to_string(&fresh)?),
          ))
          .execute(connection)?;
        diesel::insert_into(runs::table)
          .values((
            runs::id.eq(&run.id),
            runs::session_id.eq(&fresh.id),
            runs::idempotency_key.eq(input.idempotency_key),
            runs::status.eq(&run.status),
            runs::created_at_ms.eq(run.created_at_ms),
            runs::record.eq(serde_json::to_string(&run)?),
          ))
          .execute(connection)?;
        event_tx(
          connection,
          &run.id,
          "run_accepted",
          serde_json::json!({"run_id":run.id,"recovery_of":original.id}),
        )?;
        // Harness snapshots, tool frames and approvals are deliberately not copied.
        resolution.session_id = Some(fresh.id);
        resolution.replacement_run_id = Some(run.id);
      }
      // Revoke late callbacks before accepting the human resolution. The original
      // Run and Harness snapshot remain unchanged, including uncertain evidence.
      diesel::update(sessions::table.find(&session.id))
        .set((
          sessions::owner_epoch.eq(i64::try_from(session.owner_epoch)?),
          sessions::record.eq(serde_json::to_string(&session)?),
        ))
        .execute(connection)?;
      diesel::insert_into(run_recoveries::table)
        .values((
          run_recoveries::run_id.eq(&original.id),
          run_recoveries::idempotency_key.eq(input.idempotency_key),
          run_recoveries::record.eq(serde_json::to_string(&resolution)?),
        ))
        .execute(connection)
        .context("persist recovery decision")?;
      event_tx(
        connection,
        &original.id,
        "run_recovery_recorded",
        serde_json::to_value(&resolution)?,
      )?;
      Ok((resolution, true))
    })
  }
}
