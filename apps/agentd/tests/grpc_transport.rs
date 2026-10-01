use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentCommand, ModelSelection, RuntimePhase, RuntimeSnapshot};
use agent_client::{AgentClient, ApprovalDecision as ClientApprovalDecision, Command};
use agent_protocol::v1::agent_service_client::AgentServiceClient as RawAgentClient;
use agent_protocol::v1::{HealthRequest, RequestMetadata};
use agentd::{RuntimeHost, ServeError, VersionedSnapshot, serve};
use async_trait::async_trait;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct TestHost {
  commands: Arc<Mutex<Vec<AgentCommand>>>,
  snapshots: watch::Sender<VersionedSnapshot>,
}

impl TestHost {
  fn new() -> Self {
    let snapshot = VersionedSnapshot::new(1, RuntimeSnapshot::default());
    let (snapshots, _) = watch::channel(snapshot);
    Self {
      commands: Arc::new(Mutex::new(Vec::new())),
      snapshots,
    }
  }

  fn publish(&self, revision: u64, phase: RuntimePhase) {
    self.snapshots.send_replace(VersionedSnapshot::new(
      revision,
      RuntimeSnapshot {
        phase,
        ..RuntimeSnapshot::default()
      },
    ));
  }
}

#[async_trait]
impl RuntimeHost for TestHost {
  async fn execute(&self, command: AgentCommand) -> Result<(), agent_app::RuntimeCommandError> {
    self.commands.lock().await.push(command);
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }
}

async fn connected_client() -> (TestHost, AgentClient, CancellationToken) {
  let host = TestHost::new();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let shutdown = CancellationToken::new();
  let server_shutdown = shutdown.clone();
  let server_host = host.clone();
  tokio::spawn(async move {
    serve(listener, Arc::new(server_host), server_shutdown.cancelled())
      .await
      .unwrap();
  });
  let mut client = AgentClient::connect(format!("http://{address}"), "test-tui")
    .await
    .unwrap();
  client
    .handshake("agent-tui", env!("CARGO_PKG_VERSION"))
    .await
    .unwrap();
  (host, client, shutdown)
}

#[tokio::test]
async fn unary_command_reaches_runtime_host() {
  let (host, mut client, shutdown) = connected_client().await;

  client.start_turn("hello daemon").await.unwrap();

  assert_eq!(
    host.commands.lock().await.as_slice(),
    &[AgentCommand::StartTurn {
      text: "hello daemon".into()
    }]
  );
  shutdown.cancel();
}

#[tokio::test]
async fn snapshot_contains_runtime_state_and_revision() {
  let (host, mut client, shutdown) = connected_client().await;
  host.publish(7, RuntimePhase::Ready);

  let snapshot = client.get_snapshot().await.unwrap();

  assert_eq!(snapshot.revision, 7);
  assert_eq!(snapshot.phase_name(), "READY");
  shutdown.cancel();
}

#[tokio::test]
async fn server_stream_delivers_revisioned_runtime_events() {
  let (host, mut client, shutdown) = connected_client().await;
  let mut events = client.subscribe_events(1).await.unwrap();

  host.publish(42, RuntimePhase::Streaming);
  let event = events.message().await.unwrap().unwrap();

  assert_eq!(event.revision, 42);
  assert_eq!(event.snapshot.unwrap().phase_name(), "STREAMING");
  shutdown.cancel();
}

#[tokio::test]
async fn graceful_shutdown_completes_with_an_open_event_stream() {
  let host = TestHost::new();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let shutdown = CancellationToken::new();
  let server_shutdown = shutdown.clone();
  let server =
    tokio::spawn(async move { serve(listener, Arc::new(host), server_shutdown.cancelled()).await });
  let mut client = AgentClient::connect(format!("http://{address}"), "shutdown-test")
    .await
    .unwrap();
  client
    .handshake("agent-tui", env!("CARGO_PKG_VERSION"))
    .await
    .unwrap();
  let _open_stream = client.subscribe_events(1).await.unwrap();

  shutdown.cancel();

  tokio::time::timeout(Duration::from_secs(1), server)
    .await
    .expect("server shutdown must not wait forever for an event stream")
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn server_rejects_non_loopback_listener() {
  let host = TestHost::new();
  let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();

  let error = serve(listener, Arc::new(host), std::future::pending())
    .await
    .unwrap_err();

  assert!(matches!(error, ServeError::NonLoopback(_)));
}

#[tokio::test]
async fn client_exposes_handshake_and_every_command() {
  let (host, mut client, shutdown) = connected_client().await;
  let request_id = uuid::Uuid::new_v4();

  assert_eq!(
    client.health().await.unwrap().status,
    agent_protocol::v1::HealthStatus::Serving as i32
  );
  assert_eq!(
    client
      .handshake("agent-tui", env!("CARGO_PKG_VERSION"))
      .await
      .unwrap()
      .protocol_version,
    agent_protocol::PROTOCOL_VERSION
  );
  client.steer_current_turn("focus here").await.unwrap();
  client.queue_follow_up("then test it").await.unwrap();
  client.cancel_turn().await.unwrap();
  client.force_stop().await.unwrap();
  client
    .respond_approval(request_id.to_string(), ClientApprovalDecision::AllowOnce)
    .await
    .unwrap();
  client
    .select_session_model(ModelSelection::Pinned {
      provider: "local".into(),
      model: "worker".into(),
    })
    .await
    .unwrap();

  let commands = host.commands.lock().await;
  assert!(matches!(
    commands.as_slice(),
    [
      AgentCommand::SteerCurrentTurn { text: steer },
      AgentCommand::QueueFollowUp { text: queued },
      AgentCommand::CancelTurn,
      AgentCommand::ForceStop,
      AgentCommand::RespondApproval { request_id: actual, decision: agent_app::ApprovalDecision::AllowOnce },
      AgentCommand::SelectSessionModel { selection: ModelSelection::Pinned { provider, model } }
    ] if steer == "focus here" && queued == "then test it" && actual.as_uuid() == request_id
      && provider == "local" && model == "worker"
  ));
  shutdown.cancel();
}

#[tokio::test]
async fn duplicate_command_id_is_executed_once() {
  let (host, mut client, shutdown) = connected_client().await;
  let command = Command::StartTurn {
    text: "only once".into(),
  };

  client
    .execute_with_id("same-command", command.clone())
    .await
    .unwrap();
  client
    .execute_with_id("same-command", command)
    .await
    .unwrap();

  assert_eq!(host.commands.lock().await.len(), 1);
  shutdown.cancel();
}

#[tokio::test]
async fn incompatible_protocol_version_reports_supported_version() {
  let host = TestHost::new();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let shutdown = CancellationToken::new();
  let server_shutdown = shutdown.clone();
  tokio::spawn(async move {
    serve(listener, Arc::new(host), server_shutdown.cancelled())
      .await
      .unwrap();
  });
  let mut client = RawAgentClient::connect(format!("http://{address}"))
    .await
    .unwrap();

  let error = client
    .health(HealthRequest {
      metadata: Some(RequestMetadata {
        protocol_version: 99,
        client_instance_id: "future-gui".into(),
        command_id: "health-1".into(),
        expected_server_instance_id: String::new(),
      }),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::FailedPrecondition);
  assert!(
    error
      .message()
      .contains("server supports protocol_version 1")
  );
  shutdown.cancel();
}
