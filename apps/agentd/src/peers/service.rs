use std::sync::{Arc, atomic::Ordering};

use agent_distributed::worker::TaskWorker;
use agent_protocol::v1::{self, task_service_server::TaskService};
use tonic::{Request, Response, Status};

use crate::Daemon;

/// Submission shares the daemon's upgrade/shutdown fence, including queued attempts.
pub(super) struct WorkerService {
  pub daemon: Daemon,
  pub worker: Arc<TaskWorker>,
}

#[tonic::async_trait]
impl TaskService for WorkerService {
  async fn submit_task(
    &self,
    request: Request<v1::TaskEnvelope>,
  ) -> Result<Response<v1::Task>, Status> {
    let _submission = self.daemon.submission.lock().await;
    if self.daemon.stopping.is_cancelled() || self.daemon.draining.load(Ordering::Acquire) {
      return Err(Status::unavailable("daemon is draining or shutting down"));
    }
    self.worker.submit_task(request).await
  }
  async fn get_task(
    &self,
    request: Request<v1::TaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    self.worker.get_task(request).await
  }
  type WatchTaskStream = <TaskWorker as TaskService>::WatchTaskStream;
  async fn watch_task(
    &self,
    request: Request<v1::WatchRequest>,
  ) -> Result<Response<Self::WatchTaskStream>, Status> {
    self.worker.watch_task(request).await
  }
  async fn cancel_task(
    &self,
    request: Request<v1::TaskRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    self.worker.cancel_task(request).await
  }
  async fn accept_result(
    &self,
    request: Request<v1::AcceptResultRequest>,
  ) -> Result<Response<v1::Task>, Status> {
    self.worker.accept_result(request).await
  }
}
