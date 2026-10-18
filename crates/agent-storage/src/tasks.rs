use crate::{
  Store, event_tx,
  schema::{peers, tasks},
};
use agent_core::{PeerTrust, Task, TaskEnvelope};
use anyhow::Result;
use diesel::prelude::*;
impl Store {
  pub fn accept_task(&self, envelope: TaskEnvelope) -> Result<(Task, bool)> {
    self
      .lock()?
      .immediate_transaction(|connection| accept_task_tx(connection, envelope, None))
  }
  pub fn accept_dispatched_task(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
  ) -> Result<(Task, bool)> {
    self.lock()?.immediate_transaction(|connection| {
      accept_task_tx(connection, envelope, Some(worker_node_id))
    })
  }
  pub fn task_target(&self, task_id: &str) -> Result<Option<String>> {
    let Some(event) = self.events(task_id, 0, 1)?.into_iter().next() else {
      return Ok(None);
    };
    anyhow::ensure!(
      event.kind == "task_accepted",
      "task acceptance intent is missing"
    );
    let payload: serde_json::Value = serde_json::from_str(&event.payload_json)?;
    Ok(payload["worker_node_id"].as_str().map(str::to_owned))
  }
  pub fn accept_worker_task(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
    workspace: &str,
  ) -> Result<(Task, agent_core::Session, bool)> {
    let local_session_id = format!("worker:{}:{}", envelope.task_id, envelope.attempt_id);
    self.lock()?.immediate_transaction(|connection| {
      let (task, created) = accept_task_tx(connection, envelope, Some(worker_node_id))?;
      let session = if created {
        let session = agent_core::Session {
          id: local_session_id.clone(),
          title: "Worker task".into(),
          workspace: workspace.into(),
          owner_node_id: worker_node_id.into(),
          owner_epoch: 1,
          created_at_ms: agent_core::now_ms(),
        };
        diesel::insert_into(crate::schema::sessions::table)
          .values((
            crate::schema::sessions::id.eq(&session.id),
            crate::schema::sessions::owner_epoch.eq(1_i64),
            crate::schema::sessions::record.eq(serde_json::to_string(&session)?),
          ))
          .execute(connection)?;
        event_tx(
          connection,
          &task.envelope.task_id,
          "worker_session_created",
          serde_json::json!({"local_session_id":local_session_id}),
        )?;
        session
      } else {
        let record: String = crate::schema::sessions::table
          .find(&local_session_id)
          .select(crate::schema::sessions::record)
          .first(connection)?;
        serde_json::from_str(&record)?
      };
      Ok((task, session, created))
    })
  }
  pub fn task(&self, id: &str) -> Result<Option<Task>> {
    let json: Option<String> = tasks::table
      .find(id)
      .select(tasks::record)
      .first(&mut *self.lock()?)
      .optional()?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub fn tasks(&self) -> Result<Vec<Task>> {
    let records = tasks::table
      .select(tasks::record)
      .load::<String>(&mut *self.lock()?)?;
    crate::decode(records)
  }
  pub fn mark_task_running(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self.mutate_task(id, attempt, epoch, |task| {
      anyhow::ensure!(
        task.attempt_status == "accepted",
        "attempt cannot start from current state"
      );
      task.status = "running".into();
      task.attempt_status = "running".into();
      Ok(())
    })
  }
  pub fn mark_task_uncertain(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self.mutate_task(id, attempt, epoch, |task| {
      anyhow::ensure!(
        matches!(task.attempt_status.as_str(), "accepted" | "running"),
        "attempt is not in flight"
      );
      task.status = "uncertain".into();
      task.attempt_status = "uncertain".into();
      Ok(())
    })
  }
  pub fn mark_task_cancelled(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self.mutate_task(id, attempt, epoch, |task| {
      anyhow::ensure!(
        matches!(
          task.attempt_status.as_str(),
          "accepted" | "running" | "uncertain"
        ),
        "attempt is not cancellable"
      );
      task.status = "cancelled".into();
      task.attempt_status = "cancelled".into();
      Ok(())
    })
  }
  pub fn finish_attempt(
    &self,
    id: &str,
    attempt: &str,
    epoch: u64,
    result: &str,
    error: &str,
  ) -> Result<Task> {
    self.mutate_task(id, attempt, epoch, |task| {
      anyhow::ensure!(
        matches!(
          task.attempt_status.as_str(),
          "accepted" | "running" | "uncertain"
        ),
        "attempt already terminal"
      );
      task.attempt_status = if error.is_empty() {
        "succeeded"
      } else {
        "failed"
      }
      .into();
      task.status = if error.is_empty() {
        "awaiting_acceptance"
      } else {
        "failed"
      }
      .into();
      task.result = result.into();
      task.error = error.into();
      Ok(())
    })
  }
  pub fn accept_result(&self, id: &str, attempt: &str, epoch: u64, accepted: bool) -> Result<Task> {
    self.mutate_task(id, attempt, epoch, |task| {
      anyhow::ensure!(
        task.status == "awaiting_acceptance" && task.attempt_status == "succeeded",
        "attempt has no successful result to accept"
      );
      task.status = if accepted { "completed" } else { "rejected" }.into();
      Ok(())
    })
  }
  pub fn cancel_task(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self.mutate_task(id, attempt, epoch, |task| {
      anyhow::ensure!(
        !matches!(
          task.status.as_str(),
          "completed" | "rejected" | "failed" | "cancelled"
        ),
        "task already terminal"
      );
      task.status = "cancel_requested".into();
      Ok(())
    })
  }
  fn mutate_task(
    &self,
    id: &str,
    attempt: &str,
    epoch: u64,
    f: impl FnOnce(&mut Task) -> Result<()>,
  ) -> Result<Task> {
    self.lock()?.immediate_transaction(|connection| {
      let json: String = tasks::table.find(id).select(tasks::record).first(connection)?;
      let mut task: Task = serde_json::from_str(&json)?;
      anyhow::ensure!(task.envelope.owner_epoch == epoch && task.envelope.attempt_id == attempt,"stale task attempt or epoch");
      let local_session: Option<String> = crate::schema::sessions::table.find(&task.envelope.session_id)
        .select(crate::schema::sessions::record).first(connection).optional()?;
      if let Some(record) = local_session {
        let session: agent_core::Session = serde_json::from_str(&record)?;
        anyhow::ensure!(session.owner_epoch == epoch && session.owner_node_id == task.envelope.owner_node_id,
          "task session ownership changed");
      }
      f(&mut task)?;
      let event = event_tx(connection,id,"task_updated",serde_json::json!({"status":task.status,"attempt_status":task.attempt_status,"result":task.result,"error":task.error}))?;
      task.cursor = event.cursor;
      diesel::update(tasks::table.find(id)).set(tasks::record.eq(serde_json::to_string(&task)?)).execute(connection)?;
      Ok(task)
    })
  }
  pub fn trust_peer(&self, value: &PeerTrust) -> Result<()> {
    let record = serde_json::to_string(value)?;
    self.lock()?.immediate_transaction(|connection| {
      diesel::insert_into(peers::table)
        .values((peers::node_id.eq(&value.node_id), peers::record.eq(&record)))
        .on_conflict(peers::node_id)
        .do_update()
        .set(peers::record.eq(&record))
        .execute(connection)?;
      event_tx(
        connection,
        &format!("peer:{}", value.node_id),
        "peer_trust_changed",
        serde_json::json!({"revoked":value.revoked,"capabilities":value.capabilities}),
      )?;
      Ok(())
    })
  }
  pub fn peers(&self) -> Result<Vec<PeerTrust>> {
    let records = peers::table
      .select(peers::record)
      .load::<String>(&mut *self.lock()?)?;
    crate::decode(records)
  }
  pub fn peer(&self, id: &str) -> Result<Option<PeerTrust>> {
    let json: Option<String> = peers::table
      .find(id)
      .select(peers::record)
      .first(&mut *self.lock()?)
      .optional()?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
}

pub(crate) fn accept_task_tx(
  connection: &mut diesel::SqliteConnection,
  envelope: TaskEnvelope,
  worker_node_id: Option<&str>,
) -> Result<(Task, bool)> {
  anyhow::ensure!(
    !envelope.task_id.is_empty()
      && !envelope.attempt_id.is_empty()
      && !envelope.idempotency_key.is_empty(),
    "task identity is required"
  );
  anyhow::ensure!(envelope.owner_epoch > 0, "invalid owner epoch");
  let existing: Option<String> = tasks::table
    .filter(
      tasks::id
        .eq(&envelope.task_id)
        .or(tasks::idempotency_key.eq(&envelope.idempotency_key)),
    )
    .select(tasks::record)
    .first(connection)
    .optional()?;
  if let Some(json) = existing {
    let value: Task = serde_json::from_str(&json)?;
    anyhow::ensure!(value.envelope == envelope, "conflicting task resubmission");
    return Ok((value, false));
  }
  let mut task = Task {
    envelope,
    status: "accepted".into(),
    attempt_status: "accepted".into(),
    result: String::new(),
    error: String::new(),
    cursor: 0,
  };
  let event = event_tx(
    connection,
    &task.envelope.task_id,
    "task_accepted",
    serde_json::json!({"attempt_id":task.envelope.attempt_id,"worker_node_id":worker_node_id}),
  )?;
  task.cursor = event.cursor;
  diesel::insert_into(tasks::table)
    .values((
      tasks::id.eq(&task.envelope.task_id),
      tasks::idempotency_key.eq(&task.envelope.idempotency_key),
      tasks::owner_epoch.eq(i64::try_from(task.envelope.owner_epoch)?),
      tasks::record.eq(serde_json::to_string(&task)?),
    ))
    .execute(connection)?;
  Ok((task, true))
}
