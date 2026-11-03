use crate::task_acceptance::accept_task_tx;
use crate::{
  Store, event_tx,
  schema::{sessions, tasks},
};
use agent_core::{Task, TaskEnvelope};
use anyhow::Result;
use sea_orm::{QuerySelect, Set, prelude::*};
impl Store {
  pub async fn accept_task(&self, envelope: TaskEnvelope) -> Result<(Task, bool)> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> =
        async { accept_task_tx(connection, envelope, None, None).await }.await;
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
  pub async fn accept_dispatched_task(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
  ) -> Result<(Task, bool)> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> =
        async { accept_task_tx(connection, envelope, Some(worker_node_id), None).await }.await;
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
  pub async fn accept_worker_task(
    &self,
    envelope: TaskEnvelope,
    worker_node_id: &str,
    workspace: &str,
  ) -> Result<(Task, agent_core::Session, bool)> {
    let local_session_id = format!("worker:{}:{}", envelope.task_id, envelope.attempt_id);
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let (task, created) =
          accept_task_tx(connection, envelope, Some(worker_node_id), None).await?;
        let session = if created {
          let session = agent_core::Session {
            id: local_session_id.clone(),
            title: "Worker task".into(),
            workspace: workspace.into(),
            owner_node_id: worker_node_id.into(),
            owner_epoch: 1,
            created_at_ms: agent_core::now_ms(),
          };
          sessions::ActiveModel {
            id: Set(session.id.to_owned()),
            owner_epoch: Set((1_i64).to_owned()),
            record: Set((serde_json::to_string(&session)?).to_owned()),
          }
          .insert(connection)
          .await?;
          event_tx(
            connection,
            &task.envelope.task_id,
            "worker_session_created",
            serde_json::json!({"local_session_id":local_session_id}),
          )
          .await?;
          session
        } else {
          let record: String = sessions::Entity::find()
            .filter(sessions::Column::Id.eq(&local_session_id))
            .select_only()
            .column(sessions::Column::Record)
            .into_tuple::<String>()
            .one(connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("sessions record missing"))?;
          serde_json::from_str(&record)?
        };
        Ok((task, session, created))
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
  pub async fn task(&self, id: &str) -> Result<Option<Task>> {
    let json: Option<String> = tasks::Entity::find()
      .filter(tasks::Column::Id.eq(id))
      .select_only()
      .column(tasks::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub async fn tasks(&self) -> Result<Vec<Task>> {
    let records = tasks::Entity::find()
      .select_only()
      .column(tasks::Column::Record)
      .into_tuple::<String>()
      .all(self.connection())
      .await?;
    crate::decode(records)
  }
  pub async fn mark_task_running(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self
      .mutate_task(id, attempt, epoch, |task| {
        anyhow::ensure!(
          task.attempt_status == "accepted",
          "attempt cannot start from current state"
        );
        task.status = "running".into();
        task.attempt_status = "running".into();
        Ok(())
      })
      .await
  }
  pub async fn mark_task_uncertain(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self
      .mutate_task(id, attempt, epoch, |task| {
        anyhow::ensure!(
          matches!(task.attempt_status.as_str(), "accepted" | "running"),
          "attempt is not in flight"
        );
        task.status = "uncertain".into();
        task.attempt_status = "uncertain".into();
        Ok(())
      })
      .await
  }
  pub async fn mark_task_cancelled(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self
      .mutate_task(id, attempt, epoch, |task| {
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
      .await
  }
  pub async fn finish_attempt(
    &self,
    id: &str,
    attempt: &str,
    epoch: u64,
    result: &str,
    error: &str,
  ) -> Result<Task> {
    self
      .mutate_task(id, attempt, epoch, |task| {
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
      .await
  }
  pub async fn accept_result(
    &self,
    id: &str,
    attempt: &str,
    epoch: u64,
    accepted: bool,
  ) -> Result<Task> {
    self
      .mutate_task(id, attempt, epoch, |task| {
        anyhow::ensure!(
          task.status == "awaiting_acceptance" && task.attempt_status == "succeeded",
          "attempt has no successful result to accept"
        );
        task.status = if accepted { "completed" } else { "rejected" }.into();
        Ok(())
      })
      .await
  }
  pub async fn cancel_task(&self, id: &str, attempt: &str, epoch: u64) -> Result<Task> {
    self
      .mutate_task(id, attempt, epoch, |task| {
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
      .await
  }
  async fn mutate_task(
    &self,
    id: &str,
    attempt: &str,
    epoch: u64,
    f: impl FnOnce(&mut Task) -> Result<()>,
  ) -> Result<Task> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
      let json: String = tasks::Entity::find()
        .filter(tasks::Column::Id.eq(id))
        .select_only()
        .column(tasks::Column::Record)
        .into_tuple::<String>()
        .one(connection)
        .await?
        .ok_or_else(|| anyhow::anyhow!("tasks record missing"))?;
      let mut task: Task = serde_json::from_str(&json)?;
      anyhow::ensure!(
        task.envelope.owner_epoch == epoch && task.envelope.attempt_id == attempt,
        "stale task attempt or epoch"
      );
      let local_session: Option<String> = sessions::Entity::find()
        .filter(sessions::Column::Id.eq(&task.envelope.session_id))
        .select_only()
        .column(sessions::Column::Record)
        .into_tuple::<String>()
        .one(connection)
        .await?;
      if let Some(record) = local_session {
        let session: agent_core::Session = serde_json::from_str(&record)?;
        anyhow::ensure!(
          session.owner_epoch == epoch && session.owner_node_id == task.envelope.owner_node_id,
          "task session ownership changed"
        );
      }
      f(&mut task)?;
      let event = event_tx(connection,id,"task_updated",serde_json::json!({"status":task.status,"attempt_status":task.attempt_status,"result":task.result,"error":task.error})).await?;
      task.cursor = event.cursor;
      tasks::Entity::update_many()
        .col_expr(tasks::Column::Status, Expr::value(task.status.clone()))
        .col_expr(
          tasks::Column::AttemptStatus,
          Expr::value(task.attempt_status.clone()),
        )
        .filter(tasks::Column::Id.eq(id))
        .col_expr(
          tasks::Column::Record,
          Expr::value((serde_json::to_string(&task)?).to_owned()),
        )
        .exec(connection)
        .await
        .map(|result| result.rows_affected)?;
      Ok(task)
    }.await;
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
