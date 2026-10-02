use std::pin::Pin;
use std::sync::{Arc, Mutex};

use agent_local::{
  AUTHORIZATION_METADATA_KEY, DiscoveryDescriptor, ScopeFingerprint, SecretToken,
  bearer_authorization,
};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::agent_service_server::{AgentService, AgentServiceServer};
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{
  CancelModelDownloadRequest, CancelTurnRequest, CommandResponse, ForceStopRequest,
  GetSnapshotRequest, HandshakeRequest, HandshakeResponse, HealthRequest, HealthResponse,
  QueueFollowUpRequest, RespondApprovalRequest, RuntimeEvent, RuntimePhase, RuntimeSnapshot,
  SelectSessionModelRequest, StartModelDownloadRequest, StartTurnRequest, SteerCurrentTurnRequest,
  SubscribeEventsRequest,
};
use futures::{Stream, stream};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use agent_client::AuthenticatedTarget;

const SCOPE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
type EventStream = Pin<Box<dyn Stream<Item = Result<RuntimeEvent, Status>> + Send>>;

#[derive(Clone)]
struct EpochService {
  server_instance_id: String,
  protocol_version: u32,
  phase: RuntimePhase,
  commands: Arc<Mutex<Vec<String>>>,
  stream_shutdown: CancellationToken,
}

#[tonic::async_trait]
impl AgentService for EpochService {
  type SubscribeEventsStream = EventStream;

  async fn health(
    &self,
    _request: Request<HealthRequest>,
  ) -> Result<Response<HealthResponse>, Status> {
    Ok(Response::new(HealthResponse {
      protocol_version: self.protocol_version,
      status: 1,
    }))
  }

  async fn handshake(
    &self,
    _request: Request<HandshakeRequest>,
  ) -> Result<Response<HandshakeResponse>, Status> {
    Ok(Response::new(HandshakeResponse {
      protocol_version: self.protocol_version,
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
    let request = request.into_inner();
    let metadata = request
      .metadata
      .ok_or_else(|| Status::invalid_argument("metadata"))?;
    if metadata.expected_server_instance_id != self.server_instance_id {
      return Err(Status::failed_precondition("wrong epoch"));
    }
    self.commands.lock().unwrap().push(request.text);
    Ok(Response::new(CommandResponse {
      protocol_version: PROTOCOL_VERSION,
      command_id: metadata.command_id,
      accepted: true,
    }))
  }

  async fn get_snapshot(
    &self,
    _request: Request<GetSnapshotRequest>,
  ) -> Result<Response<RuntimeSnapshot>, Status> {
    Ok(Response::new(snapshot(self.phase)))
  }

  async fn subscribe_events(
    &self,
    _request: Request<SubscribeEventsRequest>,
  ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
    let shutdown = self.stream_shutdown.clone();
    let events = stream::unfold(shutdown, |shutdown| async move {
      shutdown.cancelled().await;
      None
    });
    Ok(Response::new(Box::pin(events)))
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

pub struct TestEpoch {
  pub target: AuthenticatedTarget,
  pub server_instance_id: String,
  pub commands: Arc<Mutex<Vec<String>>>,
  pub stream_shutdown: CancellationToken,
  shutdown: CancellationToken,
  task: JoinHandle<()>,
}

impl TestEpoch {
  pub async fn spawn(phase: RuntimePhase, protocol_version: u32) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let token = SecretToken::generate().unwrap();
    let scope = ScopeFingerprint::parse(SCOPE).unwrap();
    let identity = Uuid::new_v4();
    let descriptor =
      DiscoveryDescriptor::new(address, std::process::id(), identity, token.clone(), scope)
        .unwrap();
    let target = AuthenticatedTarget::from_descriptor(&descriptor);
    let commands = Arc::new(Mutex::new(Vec::new()));
    let stream_shutdown = CancellationToken::new();
    let service = EpochService {
      server_instance_id: identity.to_string(),
      protocol_version,
      phase,
      commands: Arc::clone(&commands),
      stream_shutdown: stream_shutdown.clone(),
    };
    let expected = bearer_authorization(&token);
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let task = tokio::spawn(async move {
      Server::builder()
        .add_service(AgentServiceServer::with_interceptor(
          service,
          move |request: Request<()>| {
            let accepted = request
              .metadata()
              .get(AUTHORIZATION_METADATA_KEY)
              .is_some_and(|value| value.as_bytes() == expected.as_bytes());
            if accepted {
              Ok(request)
            } else {
              Err(Status::unauthenticated("invalid bearer token"))
            }
          },
        ))
        .serve_with_incoming_shutdown(
          TcpListenerStream::new(listener),
          server_shutdown.cancelled_owned(),
        )
        .await
        .unwrap();
    });
    Self {
      target,
      server_instance_id: identity.to_string(),
      commands,
      stream_shutdown,
      shutdown,
      task,
    }
  }
}

impl Drop for TestEpoch {
  fn drop(&mut self) {
    self.shutdown.cancel();
    self.task.abort();
  }
}

fn snapshot(phase: RuntimePhase) -> RuntimeSnapshot {
  RuntimeSnapshot {
    protocol_version: PROTOCOL_VERSION,
    revision: 1,
    phase: phase as i32,
    model_selection: Some(agent_protocol::v1::ModelSelection {
      selection: Some(Selection::Auto(true)),
    }),
    ..RuntimeSnapshot::default()
  }
}
