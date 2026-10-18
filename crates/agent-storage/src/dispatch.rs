//! Master task acceptance is fenced atomically with its authoritative Session and Run.
use crate::{
  Store,
  schema::{runs, sessions, tasks},
  tasks::accept_task_tx,
  verify_epoch,
};
use agent_core::{Session, Task, TaskEnvelope};
use anyhow::{Result, ensure};
use diesel::prelude::*;
use diesel::sql_types::{Nullable, Text};

diesel::define_sql_function! {
  fn json_extract(value: Text, path: Text) -> Nullable<Text>;
}

impl Store {
  pub fn active_master_tasks(&self, node: &str) -> Result<usize> {
    let count: i64 = tasks::table
      .filter(json_extract(tasks::record, "$.envelope.owner_node_id").eq(node))
      .filter(json_extract(tasks::record, "$.attempt_status").eq_any([
        "accepted",
        "running",
        "uncertain",
      ]))
      .count()
      .get_result(&mut *self.lock()?)?;
    Ok(usize::try_from(count)?)
  }

  pub fn accept_master_dispatched_task(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
  ) -> Result<(Task, bool)> {
    self.lock()?.immediate_transaction(|connection| {
      verify_epoch(connection, &envelope.session_id, envelope.owner_epoch)?;
      let record: String = sessions::table
        .find(&envelope.session_id)
        .select(sessions::record)
        .first(connection)?;
      let session: Session = serde_json::from_str(&record)?;
      ensure!(
        session.owner_node_id == envelope.owner_node_id && session.workspace == envelope.workspace,
        "task scope differs from the authoritative Session"
      );
      let (parent_session, status): (String, String) = runs::table
        .find(&envelope.parent_run_id)
        .select((runs::session_id, runs::status))
        .first(connection)?;
      ensure!(
        parent_session == envelope.session_id && status == "running",
        "task requires an active, uncancelled parent Run in the same Session"
      );
      accept_task_tx(connection, envelope, Some(worker_node_id))
    })
  }

  /// Stable task-ID pagination; caller requests one extra row to determine continuation.
  pub fn tasks_for_session(
    &self,
    session: &str,
    after_task_id: &str,
    limit: usize,
  ) -> Result<Vec<Task>> {
    ensure!((1..=501).contains(&limit), "task page limit must be 1..501");
    let records = tasks::table
      .filter(json_extract(tasks::record, "$.envelope.session_id").eq(session))
      .filter(tasks::id.gt(after_task_id))
      .order(tasks::id.asc())
      .limit(i64::try_from(limit)?)
      .select(tasks::record)
      .load::<String>(&mut *self.lock()?)?;
    crate::decode(records)
  }
}
