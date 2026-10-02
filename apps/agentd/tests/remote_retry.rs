mod support;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_app::{
  AgentClient as _, AgentClientError, AgentCommand, ProtocolErrorKind, RuntimeSnapshot,
};
use agent_client::{RemoteAgentClient, RemoteClientOptions};
use agent_protocol::v1::agent_service_server::{AgentService, AgentServiceServer};
use agent_protocol::v1::*;
use agentd::VersionedSnapshot;
use futures::Stream;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};

use support::{baseline::BaselineService, test_target};

#[derive(Clone, Copy)]
enum FailureMode {
  OnceUnavailable,
  AlwaysInvalid,
  BlockingStart,
}

#[derive(Clone)]
struct FlakyService {
  inner: BaselineService,
  mode: FailureMode,
  attempts: Arc<AtomicUsize>,
  command_ids: Arc<Mutex<Vec<String>>>,
  command_epochs: Arc<Mutex<Vec<String>>>,
  start_gate: Arc<Notify>,
  cancel_seen: Arc<Notify>,
}

#[tonic::async_trait]
impl AgentService for FlakyService {
  type SubscribeEventsStream = Pin<Box<dyn Stream<Item = Result<RuntimeEvent, Status>> + Send>>;

  async fn health(
    &self,
    request: Request<HealthRequest>,
  ) -> Result<Response<HealthResponse>, Status> {
    self.inner.health(request).await
  }

  async fn handshake(
    &self,
    request: Request<HandshakeRequest>,
  ) -> Result<Response<HandshakeResponse>, Status> {
    self.inner.handshake(request).await
  }

  async fn start_turn(
    &self,
    request: Request<StartTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    let command_id = request
      .get_ref()
      .metadata
      .as_ref()
      .map(|metadata| metadata.command_id.clone())
      .unwrap_or_default();
    self.command_ids.lock().await.push(command_id);
    let command_epoch = request
      .get_ref()
      .metadata
      .as_ref()
      .map(|metadata| metadata.expected_server_instance_id.clone())
      .unwrap_or_default();
    self.command_epochs.lock().await.push(command_epoch);
    let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
    match self.mode {
      FailureMode::OnceUnavailable if attempt == 0 => Err(Status::unavailable("retry me")),
      FailureMode::AlwaysInvalid => Err(Status::invalid_argument("do not retry")),
      FailureMode::BlockingStart => {
        self.start_gate.notified().await;
        self.inner.start_turn(request).await
      }
      FailureMode::OnceUnavailable => self.inner.start_turn(request).await,
    }
  }

  async fn steer_current_turn(
    &self,
    request: Request<SteerCurrentTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.steer_current_turn(request).await
  }

  async fn queue_follow_up(
    &self,
    request: Request<QueueFollowUpRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.queue_follow_up(request).await
  }

  async fn cancel_turn(
    &self,
    request: Request<CancelTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.cancel_seen.notify_one();
    self.inner.cancel_turn(request).await
  }

  async fn force_stop(
    &self,
    request: Request<ForceStopRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.force_stop(request).await
  }

  async fn respond_approval(
    &self,
    request: Request<RespondApprovalRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.respond_approval(request).await
  }

  async fn select_session_model(
    &self,
    request: Request<SelectSessionModelRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.select_session_model(request).await
  }

  async fn start_model_download(
    &self,
    request: Request<StartModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.start_model_download(request).await
  }

  async fn cancel_model_download(
    &self,
    request: Request<CancelModelDownloadRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.cancel_model_download(request).await
  }

  async fn get_snapshot(
    &self,
    request: Request<GetSnapshotRequest>,
  ) -> Result<Response<agent_protocol::v1::RuntimeSnapshot>, Status> {
    self.inner.get_snapshot(request).await
  }

  async fn subscribe_events(
    &self,
    request: Request<SubscribeEventsRequest>,
  ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
    self.inner.subscribe_events(request).await
  }
}

struct TestConnection {
  client: RemoteAgentClient,
  command_ids: Arc<Mutex<Vec<String>>>,
  command_epochs: Arc<Mutex<Vec<String>>>,
  attempts: Arc<AtomicUsize>,
  start_gate: Arc<Notify>,
  cancel_seen: Arc<Notify>,
}

async fn setup(mode: FailureMode) -> TestConnection {
  let command_ids = Arc::new(Mutex::new(Vec::new()));
  let command_epochs = Arc::new(Mutex::new(Vec::new()));
  let attempts = Arc::new(AtomicUsize::new(0));
  let start_gate = Arc::new(Notify::new());
  let cancel_seen = Arc::new(Notify::new());
  let service = FlakyService {
    inner: BaselineService::new(VersionedSnapshot::new(1, RuntimeSnapshot::default())),
    mode,
    attempts: Arc::clone(&attempts),
    command_ids: Arc::clone(&command_ids),
    command_epochs: Arc::clone(&command_epochs),
    start_gate: Arc::clone(&start_gate),
    cancel_seen: Arc::clone(&cancel_seen),
  };
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(AgentServiceServer::new(service))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
      .unwrap();
  });
  let client = RemoteAgentClient::connect(
    test_target(address),
    "retry-test",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  TestConnection {
    client,
    command_ids,
    command_epochs,
    attempts,
    start_gate,
    cancel_seen,
  }
}

#[tokio::test]
async fn retryable_status_reuses_one_command_id() {
  let connection = setup(FailureMode::OnceUnavailable).await;

  connection
    .client
    .execute(AgentCommand::StartTurn {
      text: "retry".into(),
    })
    .await
    .unwrap();

  let command_ids = connection.command_ids.lock().await;
  assert_eq!(command_ids.len(), 2);
  assert_eq!(command_ids[0], command_ids[1]);
  let command_epochs = connection.command_epochs.lock().await;
  assert_eq!(command_epochs.len(), 2);
  assert!(!command_epochs[0].is_empty());
  assert_eq!(command_epochs[0], command_epochs[1]);
}

#[tokio::test]
async fn non_retryable_status_is_attempted_once() {
  let connection = setup(FailureMode::AlwaysInvalid).await;

  let error = connection
    .client
    .execute(AgentCommand::StartTurn {
      text: "fail".into(),
    })
    .await
    .unwrap_err();

  assert!(matches!(
    error,
    AgentClientError::Protocol {
      kind: ProtocolErrorKind::Unsupported,
      ..
    }
  ));
  assert_eq!(connection.command_ids.lock().await.len(), 1);
}

#[tokio::test]
async fn blocked_ordinary_command_does_not_block_control_command() {
  let connection = setup(FailureMode::BlockingStart).await;
  let ordinary_client = connection.client.clone();
  let ordinary = tokio::spawn(async move {
    ordinary_client
      .execute(AgentCommand::StartTurn {
        text: "blocked".into(),
      })
      .await
  });
  tokio::time::timeout(std::time::Duration::from_secs(1), async {
    while connection.attempts.load(Ordering::SeqCst) == 0 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();

  tokio::time::timeout(
    std::time::Duration::from_millis(200),
    connection.client.execute(AgentCommand::CancelTurn),
  )
  .await
  .expect("control command must not wait behind ordinary RPC")
  .unwrap();
  tokio::time::timeout(
    std::time::Duration::from_millis(200),
    connection.cancel_seen.notified(),
  )
  .await
  .unwrap();

  connection.start_gate.notify_waiters();
  ordinary.await.unwrap().unwrap();
}
