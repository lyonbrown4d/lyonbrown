use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentClientError, ProtocolErrorKind, RuntimeSnapshot};
use agent_client::{ConnectionState, RemoteAgentClient, RemoteClientOptions};
use agent_protocol::v1::agent_service_server::{AgentService, AgentServiceServer};
use agent_protocol::v1::*;
use agentd::{AgentGrpcService, RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use futures::Stream;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::{TcpListenerStream, UnboundedReceiverStream};
use tonic::{Request, Response, Status};

type EventResult = Result<RuntimeEvent, Status>;
type EventReceiver = mpsc::UnboundedReceiver<EventResult>;

#[derive(Clone)]
struct StaticHost(watch::Sender<VersionedSnapshot>);

#[async_trait]
impl RuntimeHost for StaticHost {
  async fn execute(
    &self,
    _command: agent_app::AgentCommand,
  ) -> Result<(), agent_app::RuntimeCommandError> {
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.0.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.0.subscribe()
  }
}

#[derive(Clone)]
struct EventService {
  inner: AgentGrpcService,
  events: mpsc::UnboundedSender<EventResult>,
  receiver: Arc<tokio::sync::Mutex<Option<EventReceiver>>>,
}

#[tonic::async_trait]
impl AgentService for EventService {
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

  async fn get_snapshot(
    &self,
    request: Request<GetSnapshotRequest>,
  ) -> Result<Response<agent_protocol::v1::RuntimeSnapshot>, Status> {
    self.inner.get_snapshot(request).await
  }

  async fn subscribe_events(
    &self,
    _request: Request<SubscribeEventsRequest>,
  ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
    let receiver = self
      .receiver
      .lock()
      .await
      .take()
      .ok_or_else(|| Status::failed_precondition("already subscribed"))?;
    Ok(Response::new(Box::pin(UnboundedReceiverStream::new(
      receiver,
    ))))
  }
}

#[tokio::test]
async fn mismatched_event_and_snapshot_revisions_fail_the_remote_driver() {
  let (snapshots, _) = watch::channel(VersionedSnapshot::new(80, RuntimeSnapshot::default()));
  let (events, receiver) = mpsc::unbounded_channel();
  let service = EventService {
    inner: AgentGrpcService::new(Arc::new(StaticHost(snapshots))),
    events,
    receiver: Arc::new(tokio::sync::Mutex::new(Some(receiver))),
  };
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
    format!("http://{address}"),
    "revision-mismatch",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  let mut states = client.subscribe_remote_state();
  let snapshot = agentd::snapshot_to_wire(VersionedSnapshot::new(81, RuntimeSnapshot::default()));

  service
    .events
    .send(Ok(RuntimeEvent {
      protocol_version: agent_protocol::PROTOCOL_VERSION,
      revision: 82,
      snapshot: Some(snapshot),
    }))
    .unwrap();
  tokio::time::timeout(Duration::from_secs(1), async {
    loop {
      if matches!(
        &states.borrow().connection,
        ConnectionState::Failed {
          error: AgentClientError::Protocol {
            kind: ProtocolErrorKind::InvalidSnapshot,
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

  drop(client);
  server.abort();
  assert!(server.await.unwrap_err().is_cancelled());
}
