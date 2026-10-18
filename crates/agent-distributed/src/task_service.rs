use std::pin::Pin;

use agent_protocol::v1::{self, task_service_server::TaskService};
use tokio::sync::mpsc;
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

use crate::{
  peers::require_capabilities,
  worker::{TaskWorker, WorkerJob},
};

#[tonic::async_trait]
impl TaskService for TaskWorker {
  async fn submit_task(
    &self,
    request: Request<v1::TaskEnvelope>,
  ) -> Result<Response<v1::Task>, Status> {
    let peer = self.manager.authorize(&request, &["tasks.submit"])?;
    let input = request.into_inner();
    if input.owner_node_id != peer.node_id {
      return Err(Status::permission_denied(
        "task master does not match its mutual TLS identity",
      ));
    }
    require_capabilities(
      &peer,
      &input
        .capabilities
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>(),
    )?;
    let envelope: agent_core::TaskEnvelope = input.into();
    let workspace = self
      .validate(&envelope)
      .map_err(|_| Status::invalid_argument("invalid task scope, capabilities or deadline"))?;
    let _submission = self.submission.lock().await;
    if self.active.lock().await.len() >= 32
      && self
        .manager
        .store
        .task(&envelope.task_id)
        .map_err(|_| Status::internal("task store unavailable"))?
        .is_none()
    {
      return Err(Status::resource_exhausted("worker task capacity reached"));
    }
    let (task, session, created) = self
      .manager
      .store
      .accept_worker_task(
        envelope,
        &self.manager.node.id,
        workspace
          .to_str()
          .ok_or_else(|| Status::invalid_argument("worker workspace must be UTF-8"))?,
      )
      .map_err(|_| Status::failed_precondition("task conflicts with existing durable state"))?;
    if created {
      let cancellation = CancellationToken::new();
      self
        .active
        .lock()
        .await
        .insert(task.envelope.task_id.clone(), cancellation.clone());
      self.launch(
        WorkerJob {
          task: task.envelope.clone(),
          session,
          workspace,
        },
        cancellation,
      );
    }
    tracing::info!(operation = "task.accept", task_id = %task.envelope.task_id, attempt_id = %task.envelope.attempt_id,
      owner_epoch = task.envelope.owner_epoch, created, "authorized worker task state is durable");
    Ok(Response::new(task.into()))
  }

  async fn get_task(
    &self,
    request: Request<v1::TaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    let peer = self.manager.authorize(&request, &["tasks.read"])?;
    Ok(Response::new(
      self
        .owned(&request.into_inner().task_id, &peer.node_id)?
        .into(),
    ))
  }

  type WatchTaskStream = Pin<Box<dyn Stream<Item = Result<v1::Event, Status>> + Send>>;
  async fn watch_task(
    &self,
    request: Request<v1::WatchRequest>,
  ) -> Result<Response<Self::WatchTaskStream>, Status> {
    let peer = self.manager.authorize(&request, &["tasks.read"])?;
    let input = request.into_inner();
    self.owned(&input.run_id, &peer.node_id)?;
    let (sender, receiver) = mpsc::channel(32);
    let worker = self.clone();
    tokio::spawn(async move {
      let mut cursor = input.after_cursor;
      loop {
        if !worker.manager.trust(&peer.node_id).is_ok_and(|trust| {
          trust.fingerprint == peer.fingerprint
            && require_capabilities(&trust, &["tasks.read"]).is_ok()
        }) {
          let _ = sender
            .send(Err(Status::permission_denied(
              "peer trust or capability was revoked",
            )))
            .await;
          break;
        }
        let events = match worker.manager.store.events(&input.run_id, cursor, 100) {
          Ok(events) => events,
          Err(_) => {
            let _ = sender
              .send(Err(Status::internal("task event store unavailable")))
              .await;
            break;
          }
        };
        for event in events {
          if !worker.manager.trust(&peer.node_id).is_ok_and(|trust| {
            trust.fingerprint == peer.fingerprint
              && require_capabilities(&trust, &["tasks.read"]).is_ok()
          }) {
            let _ = sender
              .send(Err(Status::permission_denied(
                "peer trust or capability was revoked",
              )))
              .await;
            return;
          }
          cursor = event.cursor;
          if sender.send(Ok(event.into())).await.is_err() {
            return;
          }
        }
        if worker
          .owned(&input.run_id, &peer.node_id)
          .is_ok_and(|task| {
            matches!(
              task.attempt_status.as_str(),
              "succeeded" | "failed" | "cancelled" | "uncertain"
            )
          })
        {
          break;
        }
        tokio::select! { _ = worker.changed.notified() => {}, _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}, _ = sender.closed() => break }
      }
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
  }

  async fn cancel_task(
    &self,
    request: Request<v1::TaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    let peer = self.manager.authorize(&request, &["tasks.cancel"])?;
    let task = self.owned(&request.into_inner().task_id, &peer.node_id)?;
    let task = self
      .manager
      .store
      .cancel_task(
        &task.envelope.task_id,
        &task.envelope.attempt_id,
        task.envelope.owner_epoch,
      )
      .map_err(|_| {
        Status::failed_precondition("task cannot be cancelled from its current state")
      })?;
    if let Some(cancellation) = self.active.lock().await.get(&task.envelope.task_id) {
      cancellation.cancel();
    }
    self.changed.notify_waiters();
    Ok(Response::new(task.into()))
  }

  async fn accept_result(
    &self,
    request: Request<v1::AcceptResultRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    let peer = self.manager.authorize(&request, &["tasks.accept"])?;
    let input = request.into_inner();
    self.owned(&input.task_id, &peer.node_id)?;
    let task = self
      .manager
      .store
      .accept_result(
        &input.task_id,
        &input.attempt_id,
        input.owner_epoch,
        input.accepted,
      )
      .map_err(|_| {
        Status::failed_precondition("task attempt is stale or has no result awaiting acceptance")
      })?;
    self.changed.notify_waiters();
    Ok(Response::new(task.into()))
  }
}
