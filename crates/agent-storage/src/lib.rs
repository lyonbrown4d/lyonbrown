//! Node-local durable state. Critical writes include an event and outbox row.
#![forbid(unsafe_code)]
mod approvals;
mod approved_browser_action;
mod approved_operation;
mod browser_actions;
mod control;
mod dispatch;
mod model_bindings;
mod operations;
mod persistence;
mod recovery;
mod runs;
mod schema;
mod sessions;
pub use model_bindings::ModelBinding;
mod tasks;
use agent_core::{Event, now_ms};
use anyhow::{Context, Result};
pub use approved_browser_action::{ApprovedBrowserAction, BrowserActionAuthorization};
pub use approved_operation::{ApprovedOperation, OperationAuthorization};
use diesel::{connection::SimpleConnection, prelude::*};
pub use operations::{OperationRecord, OperationStatus};
pub use recovery::{RecoverRun, RecoveryAction, RunRecovery};
use schema::{events, harness_sessions, outbox, runs as run_rows, sessions as session_rows};

#[derive(Insertable)]
#[diesel(table_name = events)]
struct EventRow<'a> {
  run_id: &'a str,
  cursor: i64,
  record: &'a str,
}

#[derive(Insertable)]
#[diesel(table_name = outbox)]
struct OutboxRow<'a> {
  run_id: &'a str,
  cursor: i64,
  delivered: i32,
}
use std::{
  path::Path,
  sync::{Arc, Mutex, MutexGuard},
};

#[derive(Clone)]
pub struct Store {
  connection: Arc<Mutex<SqliteConnection>>,
}
impl Store {
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    let path = path
      .as_ref()
      .to_str()
      .context("database path is not UTF-8")?;
    let mut connection = SqliteConnection::establish(path)?;
    connection.batch_execute("PRAGMA busy_timeout=5000;")?;
    connection.batch_execute(include_str!("schema.sql"))?;
    browser_actions::initialize(&mut connection)?;
    operations::initialize(&mut connection)?;
    recovery::initialize(&mut connection)?;
    Ok(Self {
      connection: Arc::new(Mutex::new(connection)),
    })
  }
  pub(crate) fn lock(&self) -> Result<MutexGuard<'_, SqliteConnection>> {
    self
      .connection
      .lock()
      .map_err(|_| anyhow::anyhow!("database lock poisoned"))
  }
  pub fn events(&self, run_id: &str, after: u64, limit: usize) -> Result<Vec<Event>> {
    let records = events::table
      .filter(events::run_id.eq(run_id))
      .filter(events::cursor.gt(i64::try_from(after)?))
      .order(events::cursor.asc())
      .limit(limit.min(1000) as i64)
      .select(events::record)
      .load::<String>(&mut *self.lock()?)?;
    decode(records)
  }
  pub fn append_event(
    &self,
    run_id: &str,
    kind: &str,
    payload: serde_json::Value,
  ) -> Result<Event> {
    self
      .lock()?
      .immediate_transaction(|connection| event_tx(connection, run_id, kind, payload))
  }
  pub fn append_event_owned(
    &self,
    run_id: &str,
    epoch: u64,
    kind: &str,
    payload: serde_json::Value,
  ) -> Result<Event> {
    self.lock()?.immediate_transaction(|connection| {
      let session: String = run_rows::table
        .find(run_id)
        .select(run_rows::session_id)
        .first(connection)?;
      verify_epoch(connection, &session, epoch)?;
      event_tx(connection, run_id, kind, payload)
    })
  }
  pub fn load_snapshot(&self, session_id: &str) -> Result<Option<Vec<u8>>> {
    harness_sessions::table
      .find(session_id)
      .select(harness_sessions::snapshot)
      .first(&mut *self.lock()?)
      .optional()
      .context("load harness snapshot")
  }
  pub fn commit_snapshot(
    &self,
    session_id: &str,
    epoch: u64,
    snapshot: &[u8],
    kind: &str,
    payload: serde_json::Value,
  ) -> Result<()> {
    self.lock()?.immediate_transaction(|connection| {
      verify_epoch(connection, session_id, epoch)?;
      let run_id: Option<String> = run_rows::table
        .filter(run_rows::session_id.eq(session_id))
        .order((run_rows::created_at_ms.desc(), run_rows::id.desc()))
        .select(run_rows::id)
        .first(connection)
        .optional()?;
      let run_id = run_id.context("session has no accepted run for harness mutation")?;
      diesel::insert_into(harness_sessions::table)
        .values((
          harness_sessions::session_id.eq(session_id),
          harness_sessions::snapshot.eq(snapshot),
        ))
        .on_conflict(harness_sessions::session_id)
        .do_update()
        .set(harness_sessions::snapshot.eq(snapshot))
        .execute(connection)?;
      event_tx(connection, &run_id, kind, payload)?;
      Ok(())
    })
  }
  pub fn pending_outbox(&self, limit: usize) -> Result<Vec<Event>> {
    let records = outbox::table
      .inner_join(
        events::table.on(
          events::run_id
            .eq(outbox::run_id)
            .and(events::cursor.eq(outbox::cursor)),
        ),
      )
      .filter(outbox::delivered.eq(0))
      .order((outbox::run_id.asc(), outbox::cursor.asc()))
      .limit(limit.min(1000) as i64)
      .select(events::record)
      .load::<String>(&mut *self.lock()?)?;
    decode(records)
  }
  pub fn acknowledge_outbox(&self, run_id: &str, cursor: u64) -> Result<()> {
    diesel::update(outbox::table.find((run_id, i64::try_from(cursor)?)))
      .set(outbox::delivered.eq(1))
      .execute(&mut *self.lock()?)?;
    Ok(())
  }
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(records: Vec<String>) -> Result<Vec<T>> {
  records
    .into_iter()
    .map(|record| Ok(serde_json::from_str(&record)?))
    .collect()
}
pub(crate) fn verify_epoch(
  connection: &mut SqliteConnection,
  session_id: &str,
  expected: u64,
) -> Result<()> {
  let actual: i64 = session_rows::table
    .find(session_id)
    .select(session_rows::owner_epoch)
    .first(connection)?;
  anyhow::ensure!(
    u64::try_from(actual)? == expected,
    "stale session owner epoch"
  );
  Ok(())
}
pub(crate) fn event_tx(
  connection: &mut SqliteConnection,
  run_id: &str,
  kind: &str,
  payload: serde_json::Value,
) -> Result<Event> {
  let previous: Option<i64> = events::table
    .filter(events::run_id.eq(run_id))
    .select(diesel::dsl::max(events::cursor))
    .first(connection)?;
  let cursor = previous
    .unwrap_or(0)
    .checked_add(1)
    .context("event cursor exhausted")?;
  let event = Event {
    cursor: u64::try_from(cursor)?,
    run_id: run_id.into(),
    kind: kind.into(),
    payload_json: serde_json::to_string(&payload)?,
    created_at_ms: now_ms(),
  };
  let record = serde_json::to_string(&event)?;
  diesel::insert_into(events::table)
    .values(EventRow {
      run_id,
      cursor,
      record: &record,
    })
    .execute(connection)?;
  diesel::insert_into(outbox::table)
    .values(OutboxRow {
      run_id,
      cursor,
      delivered: 0,
    })
    .execute(connection)?;
  Ok(event)
}
