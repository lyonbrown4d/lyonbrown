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
    self.accept_master_dispatch(envelope, worker_node_id, None)
  }

  /// Record the authenticated worker identity and requested origin with the first intent.
  pub fn accept_master_dispatched_task_at(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
    worker_endpoint: &str,
  ) -> Result<(Task, bool)> {
    self.accept_master_dispatch(envelope, worker_node_id, Some(worker_endpoint))
  }

  fn accept_master_dispatch(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
    worker_endpoint: Option<&str>,
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
      accept_task_tx(connection, envelope, Some(worker_node_id), worker_endpoint)
    })
  }

  pub fn task_target(&self, task_id: &str) -> Result<Option<String>> {
    Ok(
      self
        .task_target_intent(task_id)?
        .and_then(|payload| payload["worker_node_id"].as_str().map(str::to_owned)),
    )
  }

  /// Only the first accepted intent defines a task's original worker route.
  /// Legacy tasks have no route; later discovery or events cannot supply one.
  pub fn task_target_route(&self, task_id: &str) -> Result<Option<(String, String)>> {
    let Some(payload) = self.task_target_intent(task_id)? else {
      return Ok(None);
    };
    let Some(endpoint) = payload["worker_endpoint"].as_str() else {
      ensure!(
        payload["worker_endpoint"].is_null(),
        "invalid task worker endpoint"
      );
      return Ok(None);
    };
    let worker = payload["worker_node_id"]
      .as_str()
      .filter(|id| !id.is_empty())
      .ok_or_else(|| anyhow::anyhow!("task worker identity is missing"))?;
    ensure!(!endpoint.is_empty(), "task worker endpoint is empty");
    Ok(Some((worker.to_owned(), endpoint.to_owned())))
  }

  fn task_target_intent(&self, task_id: &str) -> Result<Option<serde_json::Value>> {
    let Some(event) = self.events(task_id, 0, 1)?.into_iter().next() else {
      return Ok(None);
    };
    ensure!(
      event.kind == "task_accepted",
      "task acceptance intent is missing"
    );
    Ok(Some(serde_json::from_str(&event.payload_json)?))
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
