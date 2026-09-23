use std::path::Path;

use agent_core::{AgentEvent, SessionId};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{SessionSummary, StoreError};

pub(crate) fn connect(path: &Path) -> Result<Connection, StoreError> {
  let connection = Connection::open(path).map_err(StoreError::from)?;
  connection
    .pragma_update(None, "journal_mode", "WAL")
    .map_err(StoreError::from)?;
  connection
    .pragma_update(None, "foreign_keys", "ON")
    .map_err(StoreError::from)?;
  Ok(connection)
}

pub(crate) fn initialize(path: &Path) -> Result<(), StoreError> {
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent).map_err(StoreError::from)?;
  }
  let connection = connect(path)?;
  connection
    .execute_batch(
      "CREATE TABLE IF NOT EXISTS sessions (
         id TEXT PRIMARY KEY,
         created_at TEXT NOT NULL,
         updated_at TEXT NOT NULL
       );
       CREATE TABLE IF NOT EXISTS events (
         session_id TEXT NOT NULL REFERENCES sessions(id),
         seq INTEGER NOT NULL,
         json TEXT NOT NULL,
         PRIMARY KEY(session_id, seq)
       );
       CREATE TABLE IF NOT EXISTS tool_audit (
         session_id TEXT NOT NULL REFERENCES sessions(id),
         seq INTEGER NOT NULL,
         call_id TEXT NOT NULL,
         ok INTEGER NOT NULL,
         json TEXT NOT NULL,
         PRIMARY KEY(session_id, seq)
       );",
    )
    .map_err(StoreError::from)
}

pub(crate) fn append_event(
  connection: &mut Connection,
  session_id: SessionId,
  event: &AgentEvent,
) -> Result<(), StoreError> {
  let json = serde_json::to_string(event).map_err(StoreError::from)?;
  let now = Utc::now().to_rfc3339();
  let id = session_id.to_string();
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(StoreError::from)?;
  transaction
    .execute(
      "INSERT OR IGNORE INTO sessions(id, created_at, updated_at) VALUES (?1, ?2, ?2)",
      params![id, now],
    )
    .map_err(StoreError::from)?;
  let seq: i64 = transaction
    .query_row(
      "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE session_id = ?1",
      params![id],
      |row| row.get(0),
    )
    .map_err(StoreError::from)?;
  transaction
    .execute(
      "INSERT INTO events(session_id, seq, json) VALUES (?1, ?2, ?3)",
      params![id, seq, json],
    )
    .map_err(StoreError::from)?;
  if let AgentEvent::ToolCompleted { result, .. } = event {
    transaction
      .execute(
        "INSERT INTO tool_audit(session_id, seq, call_id, ok, json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, seq, result.invocation_id.to_string(), result.ok, json],
      )
      .map_err(StoreError::from)?;
  }
  transaction
    .execute(
      "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
      params![id, now],
    )
    .map_err(StoreError::from)?;
  transaction.commit().map_err(StoreError::from)
}

pub(crate) fn load_events(
  path: &Path,
  session_id: SessionId,
) -> Result<Vec<AgentEvent>, StoreError> {
  let connection = connect(path)?;
  let mut statement = connection
    .prepare("SELECT json FROM events WHERE session_id = ?1 ORDER BY seq")
    .map_err(StoreError::from)?;
  let rows = statement
    .query_map(params![session_id.to_string()], |row| {
      row.get::<_, String>(0)
    })
    .map_err(StoreError::from)?;
  rows
    .map(|row| {
      let json = row.map_err(StoreError::from)?;
      serde_json::from_str(&json).map_err(StoreError::from)
    })
    .collect()
}

pub(crate) fn list_sessions(path: &Path) -> Result<Vec<SessionSummary>, StoreError> {
  let connection = connect(path)?;
  let mut statement = connection
    .prepare(
      "SELECT s.id, s.updated_at, COUNT(e.seq)
       FROM sessions s LEFT JOIN events e ON e.session_id = s.id
       GROUP BY s.id ORDER BY s.updated_at DESC",
    )
    .map_err(StoreError::from)?;
  let rows = statement
    .query_map([], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, i64>(2)?,
      ))
    })
    .map_err(StoreError::from)?;
  rows
    .map(|row| {
      let (id, updated_at, event_count) = row.map_err(StoreError::from)?;
      let id = uuid::Uuid::parse_str(&id).map_err(|error| StoreError(error.to_string()))?;
      Ok(SessionSummary {
        id: SessionId(id),
        updated_at,
        event_count: event_count
          .try_into()
          .map_err(|error: std::num::TryFromIntError| StoreError(error.to_string()))?,
      })
    })
    .collect()
}

pub(crate) fn audit_count(path: &Path, session_id: SessionId) -> Result<u64, StoreError> {
  let connection = connect(path)?;
  let count: i64 = connection
    .query_row(
      "SELECT COUNT(*) FROM tool_audit WHERE session_id = ?1",
      params![session_id.to_string()],
      |row| row.get(0),
    )
    .optional()
    .map_err(StoreError::from)?
    .unwrap_or(0);
  count
    .try_into()
    .map_err(|error: std::num::TryFromIntError| StoreError(error.to_string()))
}
