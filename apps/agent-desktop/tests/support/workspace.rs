use agent_protocol::v1::{self, workspace_service_server::WorkspaceService};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tonic::{Request, Response, Status};

/// Hold a real workspace write at the RPC boundary until the test releases it.
pub struct WorkspaceProxy {
  pub daemon: agentd::Daemon,
  pub write_gate: Option<Arc<Semaphore>>,
}

#[tonic::async_trait]
impl WorkspaceService for WorkspaceProxy {
  async fn list_files(
    &self,
    request: Request<v1::ListFilesRequest>,
  ) -> Result<Response<v1::WorkspaceFiles>, Status> {
    self.daemon.list_files(request).await
  }

  async fn read_file(
    &self,
    request: Request<v1::ReadFileRequest>,
  ) -> Result<Response<v1::WorkspaceFile>, Status> {
    self.daemon.read_file(request).await
  }

  async fn write_file(
    &self,
    request: Request<v1::WriteFileRequest>,
  ) -> Result<Response<v1::WorkspaceFile>, Status> {
    if let Some(gate) = &self.write_gate {
      // One permit releases one write; closing leaves subsequent writes ungated.
      if let Ok(permit) = gate.acquire().await {
        permit.forget();
      }
    }
    self.daemon.write_file(request).await
  }

  async fn get_diff(
    &self,
    request: Request<v1::DiffRequest>,
  ) -> Result<Response<v1::WorkspaceDiff>, Status> {
    self.daemon.get_diff(request).await
  }

  type WatchFilesStream = <agentd::Daemon as WorkspaceService>::WatchFilesStream;

  async fn watch_files(
    &self,
    request: Request<v1::SessionRequest>,
  ) -> Result<Response<Self::WatchFilesStream>, Status> {
    self.daemon.watch_files(request).await
  }
}
