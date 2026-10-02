mod support;

use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_app::{AgentClientError, ProtocolErrorKind, RuntimePhase as AppPhase, RuntimeSnapshot};
use agent_client::{NativeAgentClient, RemoteAgentClient, RemoteClientOptions};
use agent_protocol::v1::agent_service_server::{AgentService, AgentServiceServer};
use agent_protocol::v1::*;
use agentd::VersionedSnapshot;
use futures::Stream;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::{TcpListenerStream, UnboundedReceiverStream};
use tonic::{Request, Response, Status};

use support::{baseline::BaselineService, test_target};

type EventSender = mpsc::UnboundedSender<Result<RuntimeEvent, Status>>;

#[derive(Clone)]
struct EpochService {
  inner: BaselineService,
  generation: Arc<AtomicUsize>,
  streams: Arc<Mutex<Vec<EventSender>>>,
}

impl EpochService {
  fn restart(&self) {
    self.generation.store(1, Ordering::SeqCst);
    self.streams.lock().unwrap().clear();
  }

  fn make_incompatible(&self) {
    self.generation.store(2, Ordering::SeqCst);
  }
}

#[tonic::async_trait]
impl AgentService for EpochService {
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
    let mut response = self.inner.handshake(request).await?;
    if self.generation.load(Ordering::SeqCst) == 2 {
      response.get_mut().protocol_version = agent_protocol::PROTOCOL_VERSION + 1;
    }
    response.get_mut().server_instance_id =
      format!("daemon-epoch-{}", self.generation.load(Ordering::SeqCst));
    Ok(response)
  }

  async fn start_turn(
    &self,
    request: Request<StartTurnRequest>,
  ) -> Result<Response<CommandResponse>, Status> {
    self.inner.start_turn(request).await
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
    let mut response = self.inner.get_snapshot(request).await?;
    if self.generation.load(Ordering::SeqCst) == 1 {
      response.get_mut().revision = 3;
      response.get_mut().phase = agent_protocol::v1::RuntimePhase::Completed as i32;
    }
    Ok(response)
  }

  async fn subscribe_events(
    &self,
    _request: Request<SubscribeEventsRequest>,
  ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
    let (sender, receiver) = mpsc::unbounded_channel();
    self.streams.lock().unwrap().push(sender);
    Ok(Response::new(Box::pin(UnboundedReceiverStream::new(
      receiver,
    ))))
  }
}

fn epoch_service() -> EpochService {
  EpochService {
    inner: BaselineService::new(VersionedSnapshot::new(
      80,
      RuntimeSnapshot {
        phase: AppPhase::Streaming,
        ..RuntimeSnapshot::default()
      },
    )),
    generation: Arc::new(AtomicUsize::new(0)),
    streams: Arc::new(Mutex::new(Vec::new())),
  }
}

#[tokio::test]
async fn daemon_restart_starts_new_epoch_and_accepts_lower_revision() {
  let service = epoch_service();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let server_service = service.clone();
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(AgentServiceServer::new(server_service))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
      .unwrap();
  });
  let client = RemoteAgentClient::connect(
    test_target(address),
    "epoch-test",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  let first = client.remote_snapshot();
  let mut states = client.subscribe_remote_state();

  service.restart();
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let state = states.borrow().clone();
      let replaced = state.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.server_instance_id != first.server_instance_id
          && snapshot.revision == 3
          && snapshot.snapshot.phase == AppPhase::Completed
      });
      if replaced {
        break;
      }
      states.changed().await.unwrap();
    }
  })
  .await
  .unwrap();

  assert_eq!(client.revision(), 3);
  drop(client);
  server.abort();
  assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn native_driver_stops_on_incompatible_protocol() {
  let service = epoch_service();
  service.make_incompatible();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let server_service = service.clone();
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(AgentServiceServer::new(server_service))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
      .unwrap();
  });
  let client = NativeAgentClient::start(
    test_target(address),
    "native-incompatible",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  let monitor = client.driver_monitor();
  let mut states = client.subscribe_remote_state();

  tokio::time::timeout(Duration::from_secs(1), async {
    loop {
      if matches!(
        &states.borrow().connection,
        agent_client::ConnectionState::Failed {
          error: AgentClientError::Protocol {
            kind: ProtocolErrorKind::IncompatibleVersion,
            ..
          },
          ..
        }
      ) {
        break;
      }
      states.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
  tokio::time::timeout(Duration::from_secs(1), async {
    while monitor.is_running() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  let error = client
    .dispatch(agent_app::AgentCommand::CancelTurn)
    .await
    .unwrap_err();
  assert!(matches!(
    error,
    AgentClientError::Protocol {
      kind: ProtocolErrorKind::IncompatibleVersion,
      ..
    }
  ));

  drop(client);
  server.abort();
  assert!(server.await.unwrap_err().is_cancelled());
}
