use agent_protocol::v1;
use anyhow::Result;

use crate::AgentClient;

impl AgentClient {
  /// Submit once. A lost response requires reconciliation of the same identity.
  pub async fn dispatch_task(
    &self,
    target: v1::Node,
    envelope: v1::TaskEnvelope,
  ) -> Result<v1::Task> {
    Ok(
      self
        .orchestration
        .clone()
        .dispatch_task(v1::DispatchTaskRequest {
          target: Some(target),
          envelope: Some(envelope),
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn task(&self, task_id: &str) -> Result<v1::Task> {
    Ok(
      self
        .orchestration
        .clone()
        .get_task(v1::TaskRequest {
          task_id: task_id.into(),
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn list_tasks(
    &self,
    session_id: &str,
    after_task_id: &str,
    page_size: u32,
  ) -> Result<v1::Tasks> {
    Ok(
      self
        .orchestration
        .clone()
        .list_tasks(v1::ListTasksRequest {
          session_id: session_id.into(),
          after_task_id: after_task_id.into(),
          page_size,
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn reconcile_task(&self, target: v1::Node, task_id: &str) -> Result<v1::Task> {
    Ok(
      self
        .orchestration
        .clone()
        .reconcile_task(v1::RemoteTaskRequest {
          target: Some(target),
          task_id: task_id.into(),
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn cancel_task(&self, target: v1::Node, task_id: &str) -> Result<v1::Task> {
    Ok(
      self
        .orchestration
        .clone()
        .cancel_task(v1::RemoteTaskRequest {
          target: Some(target),
          task_id: task_id.into(),
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn accept_task_result(
    &self,
    target: v1::Node,
    task_id: &str,
    accepted: bool,
  ) -> Result<v1::Task> {
    Ok(
      self
        .orchestration
        .clone()
        .accept_result(v1::AcceptRemoteResultRequest {
          target: Some(target),
          task_id: task_id.into(),
          accepted,
        })
        .await?
        .into_inner(),
    )
  }
}
