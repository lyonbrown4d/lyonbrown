//! Authenticated local management of durable Master tasks.
mod validation;

use agent_distributed::dispatcher::Dispatcher;
use agent_protocol::v1::{
  self,
  orchestration_service_server::{OrchestrationService, OrchestrationServiceServer},
};
use agent_security::Authenticate;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};

use crate::Daemon;

pub fn orchestration_service(
  daemon: Daemon,
) -> InterceptedService<OrchestrationServiceServer<Daemon>, Authenticate> {
  let authentication = Authenticate::new(daemon.auth_token());
  InterceptedService::new(
    OrchestrationServiceServer::new(daemon).max_decoding_message_size(96 * 1024),
    authentication,
  )
}

fn failure(error: anyhow::Error) -> Status {
  // Payloads and credentials are never included in the management error.
  tracing::warn!(operation = "task.manage", outcome = "unconfirmed", error = %error);
  Status::failed_precondition(
    "task operation could not be confirmed; reconcile the existing task identity before further action",
  )
}

impl Daemon {
  fn dispatcher(&self) -> Result<Dispatcher, Status> {
    Ok(Dispatcher::new(
      crate::peers::manager(self).map_err(failure)?,
    ))
  }
}

#[tonic::async_trait]
impl OrchestrationService for Daemon {
  async fn dispatch_task(
    &self,
    request: Request<v1::DispatchTaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    let _submission = self.submission.lock().await;
    if self.stopping.is_cancelled() || self.draining.load(std::sync::atomic::Ordering::Acquire) {
      return Err(Status::unavailable("daemon is draining"));
    }
    let input = request.into_inner();
    let target = validation::target(input.target)?;
    let envelope = input
      .envelope
      .ok_or_else(|| Status::invalid_argument("task envelope is required"))?;
    self.validate_dispatch(&target, &envelope)?;
    let value = self
      .dispatcher()?
      .submit(&target, envelope.into())
      .await
      .map_err(failure)?;
    self.events.notify_waiters();
    // Return the Master's durable state, not an uncommitted Worker snapshot.
    let task_id = value
      .envelope
      .ok_or_else(|| Status::internal("worker omitted task identity"))?
      .task_id;
    Ok(Response::new(self.master_task(&task_id)?.into()))
  }

  async fn get_task(
    &self,
    request: Request<v1::TaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    Ok(Response::new(
      self.master_task(&request.into_inner().task_id)?.into(),
    ))
  }

  async fn list_tasks(
    &self,
    request: Request<v1::ListTasksRequest>,
  ) -> Result<Response<v1::Tasks>, Status> {
    let input = request.into_inner();
    let id = input.session_id;
    if !input.after_task_id.is_empty() {
      validation::identifier(&input.after_task_id)?;
    }
    let page_size = if input.page_size == 0 {
      100
    } else {
      input.page_size as usize
    };
    if page_size > 500 {
      return Err(Status::invalid_argument(
        "task page size must be at most 500",
      ));
    }
    let session = self.master_session(&id)?;
    let mut tasks = self
      .store
      .tasks_for_session(&id, &input.after_task_id, page_size + 1)
      .map_err(failure)?;
    let next_task_id = if tasks.len() > page_size {
      tasks.truncate(page_size);
      tasks
        .last()
        .map(|task| task.envelope.task_id.clone())
        .unwrap_or_default()
    } else {
      String::new()
    };
    let tasks = tasks
      .into_iter()
      .filter(|task| {
        task.envelope.owner_node_id == self.node_id
          && task.envelope.owner_epoch == session.owner_epoch
      })
      .map(Into::into)
      .collect();
    Ok(Response::new(v1::Tasks {
      tasks,
      next_task_id,
    }))
  }

  async fn reconcile_task(
    &self,
    request: Request<v1::RemoteTaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    let input = request.into_inner();
    let target = validation::target(input.target)?;
    self.master_task_target(&input.task_id, &target)?;
    let value = self
      .dispatcher()?
      .reconcile(&target, &input.task_id)
      .await
      .map_err(failure)?;
    self.events.notify_waiters();
    Ok(Response::new(value.into()))
  }

  async fn cancel_task(
    &self,
    request: Request<v1::RemoteTaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    let input = request.into_inner();
    let target = validation::target(input.target)?;
    self.master_task_target(&input.task_id, &target)?;
    let value = self
      .dispatcher()?
      .cancel(&target, &input.task_id)
      .await
      .map_err(failure)?;
    self.events.notify_waiters();
    Ok(Response::new(value.into()))
  }

  async fn accept_result(
    &self,
    request: Request<v1::AcceptRemoteResultRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    let input = request.into_inner();
    let target = validation::target(input.target)?;
    self.master_task_target(&input.task_id, &target)?;
    let value = self
      .dispatcher()?
      .accept(&target, &input.task_id, input.accepted)
      .await
      .map_err(failure)?;
    self.events.notify_waiters();
    Ok(Response::new(value.into()))
  }
}
