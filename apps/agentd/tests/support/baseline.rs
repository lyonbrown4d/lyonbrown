use std::pin::Pin;

use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::agent_service_server::AgentService;
use agent_protocol::v1::*;
use agentd::{VersionedSnapshot, snapshot_to_wire};
use futures::Stream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::test_scope;

#[derive(Clone)]
pub struct BaselineService {
  snapshot: VersionedSnapshot,
  server_instance_id: String,
}

impl BaselineService {
  pub fn new(snapshot: VersionedSnapshot) -> Self {
    Self {
      snapshot,
      server_instance_id: Uuid::new_v4().to_string(),
    }
  }

  fn command(
    &self,
    metadata: Option<RequestMetadata>,
  ) -> Result<Response<CommandResponse>, Status> {
    let metadata = metadata.ok_or_else(|| Status::invalid_argument("metadata is required"))?;
    Ok(Response::new(CommandResponse {
      protocol_version: PROTOCOL_VERSION,
      command_id: metadata.command_id,
      accepted: true,
    }))
  }
}

#[tonic::async_trait]
impl AgentService for BaselineService {
  type SubscribeEventsStream =
    Pin<Box<dyn Stream<Item = Result<RuntimeEvent, Status>> + Send + 'static>>;

  async fn health(
    &self,
    _request: Request<HealthRequest>,
  ) -> Result<Response<HealthResponse>, Status> {
    Ok(Response::new(HealthResponse {
      protocol_version: PROTOCOL_VERSION,
      status: HealthStatus::Serving as i32,
    }))
  }

  async fn handshake(
    &self,
    _request: Request<HandshakeRequest>,
  ) -> Result<Response<HandshakeResponse>, Status> {
    Ok(Response::new(HandshakeResponse {
      protocol_version: PROTOCOL_VERSION,
      server_version: env!("CARGO_PKG_VERSION").into(),
      server_instance_id: self.server_instance_id.clone(),
      capabilities: Vec::new(),
      scope_fingerprint: test_scope().as_str().into(),
    }))
  }

  async fn start_turn(
    &self,
    request: Request<StartTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn steer_current_turn(
    &self,
    request: Request<SteerCurrentTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn queue_follow_up(
    &self,
    request: Request<QueueFollowUpRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn cancel_turn(
    &self,
    request: Request<CancelTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn force_stop(
    &self,
    request: Request<ForceStopRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn respond_approval(
    &self,
    request: Request<RespondApprovalRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn select_session_model(
    &self,
    request: Request<SelectSessionModelRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn start_model_download(
    &self,
    request: Request<StartModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn cancel_model_download(
    &self,
    request: Request<CancelModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.command(request.into_inner().metadata)
  }

  async fn get_snapshot(
    &self,
    _request: Request<GetSnapshotRequest>,
  ) -> Result<Response<RuntimeSnapshot>, Status> {
    Ok(Response::new(snapshot_to_wire(self.snapshot.clone())))
  }

  async fn subscribe_events(
    &self,
    _request: Request<SubscribeEventsRequest>,
  ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
    Ok(Response::new(Box::pin(futures::stream::pending())))
  }
}
