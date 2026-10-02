use std::pin::Pin;
use std::sync::{Arc, Mutex};

use agent_client::AuthenticatedTarget;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint, SecretToken};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::agent_service_server::{AgentService, AgentServiceServer};
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{
  CancelModelDownloadRequest, CancelTurnRequest, CommandResponse, ForceStopRequest,
  GetSnapshotRequest, HandshakeRequest, HandshakeResponse, HealthRequest, HealthResponse,
  ModelSelection, QueueFollowUpRequest, RespondApprovalRequest, RuntimeEvent, RuntimePhase,
  RuntimeSnapshot, SelectSessionModelRequest, StartModelDownloadRequest, StartTurnRequest,
  SteerCurrentTurnRequest, SubscribeEventsRequest,
};
use futures::{Stream, stream};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use uuid::Uuid;

const SCOPE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
type EventStream = Pin<Box<dyn Stream<Item = Result<RuntimeEvent, Status>> + Send>>;

#[derive(Clone)]
struct EpochService {
  server_instance_id: String,
  commands: Arc<Mutex<Vec<String>>>,
}

#[tonic::async_trait]
impl AgentService for EpochService {
  type SubscribeEventsStream = EventStream;

  async fn health(
    &self,
    _request: Request<HealthRequest>,
  ) -> Result<Response<HealthResponse>, Status> {
    Ok(Response::new(HealthResponse {
      protocol_version: PROTOCOL_VERSION,
      status: 1,
    }))
  }

  async fn handshake(
    &self,
    _request: Request<HandshakeRequest>,
  ) -> Result<Response<HandshakeResponse>, Status> {
    Ok(Response::new(HandshakeResponse {
      protocol_version: PROTOCOL_VERSION,
      server_version: "test".into(),
      server_instance_id: self.server_instance_id.clone(),
      capabilities: vec!["streaming".into()],
      scope_fingerprint: SCOPE.into(),
    }))
  }

  async fn start_turn(
    &self,
    request: Request<StartTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self
      .commands
      .lock()
      .unwrap()
      .push(request.into_inner().text);
    Err(Status::failed_precondition(
      "test command should not arrive",
    ))
  }

  async fn get_snapshot(
    &self,
    _request: Request<GetSnapshotRequest>,
  ) -> Result<Response<RuntimeSnapshot>, Status> {
    Ok(Response::new(RuntimeSnapshot {
      protocol_version: PROTOCOL_VERSION,
      revision: 1,
      phase: RuntimePhase::Ready as i32,
      model_selection: Some(ModelSelection {
        selection: Some(Selection::Auto(true)),
      }),
      ..RuntimeSnapshot::default()
    }))
  }

  async fn subscribe_events(
    &self,
    _request: Request<SubscribeEventsRequest>,
  ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
    Ok(Response::new(Box::pin(stream::pending())))
  }

  async fn steer_current_turn(
    &self,
    _request: Request<SteerCurrentTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }

  async fn queue_follow_up(
    &self,
    _request: Request<QueueFollowUpRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }

  async fn cancel_turn(
    &self,
    _request: Request<CancelTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }

  async fn force_stop(
    &self,
    _request: Request<ForceStopRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }

  async fn respond_approval(
    &self,
    _request: Request<RespondApprovalRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }

  async fn select_session_model(
    &self,
    _request: Request<SelectSessionModelRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }

  async fn start_model_download(
    &self,
    _request: Request<StartModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }

  async fn cancel_model_download(
    &self,
    _request: Request<CancelModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    Err(Status::unimplemented("test"))
  }
}

pub struct EpochServer {
  pub target: AuthenticatedTarget,
  pub server_instance_id: String,
  pub commands: Arc<Mutex<Vec<String>>>,
  task: JoinHandle<()>,
}

impl EpochServer {
  pub async fn spawn() -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let identity = Uuid::new_v4();
    let descriptor = DiscoveryDescriptor::new(
      address,
      std::process::id(),
      identity,
      SecretToken::generate().unwrap(),
      ScopeFingerprint::parse(SCOPE).unwrap(),
    )
    .unwrap();
    let commands = Arc::new(Mutex::new(Vec::new()));
    let service = EpochService {
      server_instance_id: identity.to_string(),
      commands: Arc::clone(&commands),
    };
    let task = tokio::spawn(async move {
      Server::builder()
        .add_service(AgentServiceServer::new(service))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .unwrap();
    });
    Self {
      target: AuthenticatedTarget::from_descriptor(&descriptor),
      server_instance_id: identity.to_string(),
      commands,
      task,
    }
  }
}

impl Drop for EpochServer {
  fn drop(&mut self) {
    self.task.abort();
  }
}
