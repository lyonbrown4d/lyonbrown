use agent_core::{Task, TaskEnvelope};
use agent_protocol::v1;
use anyhow::{Context, Result, ensure};

use crate::peers::PeerManager;

#[derive(Clone)]
pub struct Dispatcher {
  manager: PeerManager,
}

impl Dispatcher {
  pub fn new(manager: PeerManager) -> Self {
    Self { manager }
  }

  pub async fn submit(&self, remote: &v1::Node, envelope: TaskEnvelope) -> Result<v1::Task> {
    ensure!(
      envelope.owner_node_id == self.manager.node.id,
      "only the local master may dispatch a task"
    );
    let session = self
      .manager
      .store
      .session(&envelope.session_id)?
      .context("master session does not exist")?;
    ensure!(
      session.owner_node_id == self.manager.node.id && session.owner_epoch == envelope.owner_epoch,
      "master session ownership or epoch changed"
    );
    let mut connection = self.manager.connect(remote).await?;
    ensure!(
      envelope
        .capabilities
        .iter()
        .all(|cap| connection.node.capabilities.contains(cap)),
      "peer lacks required task capabilities"
    );
    let (task, created) = self
      .manager
      .store
      .accept_master_dispatched_task(envelope.clone(), &connection.node.id)?;
    ensure!(
      created,
      "task was already dispatched or has an unknown outcome; reconcile its existing identity instead of resubmitting"
    );
    self.manager.store.mark_task_running(
      &envelope.task_id,
      &envelope.attempt_id,
      envelope.owner_epoch,
    )?;
    match connection
      .tasks
      .submit_task(v1::TaskEnvelope::from(envelope.clone()))
      .await
    {
      Ok(response) => {
        let value = response.into_inner();
        if let Err(error) = verify(&task, &value) {
          self.manager.store.mark_task_uncertain(
            &envelope.task_id,
            &envelope.attempt_id,
            envelope.owner_epoch,
          )?;
          return Err(error);
        }
        Ok(value)
      }
      Err(_) => {
        self.manager.store.mark_task_uncertain(
          &envelope.task_id,
          &envelope.attempt_id,
          envelope.owner_epoch,
        )?;
        anyhow::bail!(
          "task submission outcome is unknown; query the same task identity before taking further action"
        )
      }
    }
  }

  pub async fn reconcile(&self, remote: &v1::Node, task_id: &str) -> Result<Task> {
    let local = self
      .manager
      .store
      .task(task_id)?
      .context("unknown local task")?;
    ensure!(
      self.manager.store.task_target(task_id)?.as_deref() == Some(remote.id.as_str()),
      "task is assigned to another worker"
    );
    ensure!(
      local.envelope.owner_node_id == self.manager.node.id,
      "task belongs to another master"
    );
    let remote_task = self
      .manager
      .connect(remote)
      .await?
      .tasks
      .get_task(v1::TaskRequest {
        task_id: task_id.into(),
      })
      .await?
      .into_inner();
    verify(&local, &remote_task)?;
    if matches!(remote_task.attempt_status.as_str(), "succeeded" | "failed")
      && matches!(
        local.attempt_status.as_str(),
        "accepted" | "running" | "uncertain"
      )
    {
      return self.manager.store.finish_attempt(
        task_id,
        &local.envelope.attempt_id,
        local.envelope.owner_epoch,
        &remote_task.result,
        &remote_task.error,
      );
    }
    if remote_task.attempt_status == "cancelled"
      && matches!(
        local.attempt_status.as_str(),
        "accepted" | "running" | "uncertain"
      )
    {
      return self.manager.store.mark_task_cancelled(
        task_id,
        &local.envelope.attempt_id,
        local.envelope.owner_epoch,
      );
    }
    Ok(local)
  }

  /// Record cancellation intent before requesting the worker's cooperative cancellation.
  pub async fn cancel(&self, remote: &v1::Node, task_id: &str) -> Result<Task> {
    ensure!(
      self.manager.store.task_target(task_id)?.as_deref() == Some(remote.id.as_str()),
      "task is assigned to another worker"
    );
    let task = self
      .manager
      .store
      .task(task_id)?
      .context("unknown local task")?;
    ensure!(
      task.envelope.owner_node_id == self.manager.node.id,
      "task belongs to another master"
    );
    let requested = self.manager.store.cancel_task(
      task_id,
      &task.envelope.attempt_id,
      task.envelope.owner_epoch,
    )?;
    let outcome = match self.manager.connect(remote).await {
      Ok(mut connection) => connection
        .tasks
        .cancel_task(v1::TaskRequest {
          task_id: task_id.into(),
        })
        .await
        .map(|_| ()),
      Err(_) => Err(tonic::Status::unavailable("peer disconnected")),
    };
    if outcome.is_err() {
      self.manager.store.mark_task_uncertain(
        task_id,
        &task.envelope.attempt_id,
        task.envelope.owner_epoch,
      )?;
      anyhow::bail!(
        "task cancellation result is unknown; reconcile the existing task before further action"
      );
    }
    Ok(requested)
  }

  /// The master's durable decision precedes the optional worker acknowledgement.
  pub async fn accept(&self, remote: &v1::Node, task_id: &str, accepted: bool) -> Result<Task> {
    ensure!(
      self.manager.store.task_target(task_id)?.as_deref() == Some(remote.id.as_str()),
      "task is assigned to another worker"
    );
    let local = self
      .manager
      .store
      .task(task_id)?
      .context("unknown local task")?;
    ensure!(
      local.envelope.owner_node_id == self.manager.node.id,
      "task belongs to another master"
    );
    let decided = self.manager.store.accept_result(
      task_id,
      &local.envelope.attempt_id,
      local.envelope.owner_epoch,
      accepted,
    )?;
    let acknowledgement = match self.manager.connect(remote).await {
      Ok(mut connection) => connection
        .tasks
        .accept_result(v1::AcceptResultRequest {
          task_id: task_id.into(),
          attempt_id: local.envelope.attempt_id.clone(),
          owner_epoch: local.envelope.owner_epoch,
          accepted,
        })
        .await
        .map(|_| ()),
      Err(_) => Err(tonic::Status::unavailable("peer disconnected")),
    };
    if acknowledgement.is_err() {
      tracing::warn!(
        operation = "task.accept",
        outcome = "worker_ack_unknown",
        "master decision is durable; reconcile worker acknowledgement without replaying task execution"
      );
    }
    Ok(decided)
  }
}

fn verify(local: &Task, remote: &v1::Task) -> Result<()> {
  let envelope: TaskEnvelope = remote
    .envelope
    .clone()
    .context("peer task omitted its envelope")?
    .into();
  ensure!(
    local.envelope == envelope,
    "peer response differs from the durable task identity and scope"
  );
  Ok(())
}
