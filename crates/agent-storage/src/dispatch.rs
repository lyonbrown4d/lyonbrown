use sea_orm::{QueryOrder, QuerySelect, prelude::*};
// Master task acceptance is fenced atomically with its authoritative Session and Run.
use crate::{
  Store,
  schema::{runs, sessions, tasks},
  task_acceptance::accept_task_tx,
  verify_epoch,
};
use agent_core::{Session, Task, TaskEnvelope};
use anyhow::{Result, ensure};

impl Store {
  pub async fn active_master_tasks(&self, node: &str) -> Result<usize> {
    let count: u64 = tasks::Entity::find()
      .filter(tasks::Column::OwnerNodeId.eq(node))
      .filter(tasks::Column::AttemptStatus.is_in(["accepted", "running", "uncertain"]))
      .count(self.connection())
      .await?;
    Ok(usize::try_from(count)?)
  }

  pub async fn accept_master_dispatched_task(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
  ) -> Result<(Task, bool)> {
    self
      .accept_master_dispatch(envelope, worker_node_id, None)
      .await
  }

  /// Record the authenticated worker identity and requested origin with the first intent.
  pub async fn accept_master_dispatched_task_at(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
    worker_endpoint: &str,
  ) -> Result<(Task, bool)> {
    self
      .accept_master_dispatch(envelope, worker_node_id, Some(worker_endpoint))
      .await
  }

  async fn accept_master_dispatch(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
    worker_endpoint: Option<&str>,
  ) -> Result<(Task, bool)> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        verify_epoch(connection, &envelope.session_id, envelope.owner_epoch).await?;
        let record: String = sessions::Entity::find()
          .filter(sessions::Column::Id.eq(&envelope.session_id))
          .select_only()
          .column(sessions::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("sessions record missing"))?;
        let session: Session = serde_json::from_str(&record)?;
        ensure!(
          session.owner_node_id == envelope.owner_node_id
            && session.workspace == envelope.workspace,
          "task scope differs from the authoritative Session"
        );
        let (parent_session, status): (String, String) = runs::Entity::find()
          .filter(runs::Column::Id.eq(&envelope.parent_run_id))
          .select_only()
          .column(runs::Column::SessionId)
          .column(runs::Column::Status)
          .into_tuple::<(String, String)>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
        ensure!(
          parent_session == envelope.session_id && status == "running",
          "task requires an active, uncancelled parent Run in the same Session"
        );
        accept_task_tx(connection, envelope, Some(worker_node_id), worker_endpoint).await
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

  pub async fn task_target(&self, task_id: &str) -> Result<Option<String>> {
    Ok(
      self
        .task_target_intent(task_id)
        .await?
        .and_then(|payload| payload["worker_node_id"].as_str().map(str::to_owned)),
    )
  }

  /// Only the first accepted intent defines a task's original worker route.
  /// Legacy tasks have no route; later discovery or events cannot supply one.
  pub async fn task_target_route(&self, task_id: &str) -> Result<Option<(String, String)>> {
    let Some(payload) = self.task_target_intent(task_id).await? else {
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

  async fn task_target_intent(&self, task_id: &str) -> Result<Option<serde_json::Value>> {
    let Some(event) = self.events(task_id, 0, 1).await?.into_iter().next() else {
      return Ok(None);
    };
    ensure!(
      event.kind == "task_accepted",
      "task acceptance intent is missing"
    );
    Ok(Some(serde_json::from_str(&event.payload_json)?))
  }

  /// Stable task-ID pagination; caller requests one extra row to determine continuation.
  pub async fn tasks_for_session(
    &self,
    session: &str,
    after_task_id: &str,
    limit: usize,
  ) -> Result<Vec<Task>> {
    ensure!((1..=501).contains(&limit), "task page limit must be 1..501");
    let records = tasks::Entity::find()
      .filter(tasks::Column::SessionId.eq(session))
      .filter(tasks::Column::Id.gt(after_task_id))
      .order_by_asc(tasks::Column::Id)
      .limit((i64::try_from(limit)?) as u64)
      .select_only()
      .column(tasks::Column::Record)
      .into_tuple::<String>()
      .all(self.connection())
      .await?;
    crate::decode(records)
  }
}
