use std::sync::Arc;

use agent_app::{AgentCommand, RuntimeSnapshot};
use agent_client::AgentClient;
use agent_protocol::MAX_GRPC_MESSAGE_BYTES;
use agentd::{RuntimeHost, VersionedSnapshot, serve};
use async_trait::async_trait;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const ABOVE_TONIC_DEFAULT: usize = 5 * 1024 * 1024;
const ABOVE_PROTOCOL_LIMIT: usize = MAX_GRPC_MESSAGE_BYTES + 1024 * 1024;

#[derive(Clone)]
struct SnapshotHost {
  snapshots: watch::Sender<VersionedSnapshot>,
}

impl SnapshotHost {
  fn with_payload(bytes: usize) -> Self {
    let snapshot = RuntimeSnapshot {
      last_error: Some("x".repeat(bytes)),
      ..RuntimeSnapshot::default()
    };
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(1, snapshot));
    Self { snapshots }
  }
}

#[async_trait]
impl RuntimeHost for SnapshotHost {
  async fn execute(&self, _command: AgentCommand) -> Result<(), agent_app::RuntimeCommandError> {
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }
}

async fn connected(payload_bytes: usize) -> (AgentClient, CancellationToken) {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let shutdown = CancellationToken::new();
  let server_shutdown = shutdown.clone();
  let host = SnapshotHost::with_payload(payload_bytes);
  tokio::spawn(async move {
    serve(listener, Arc::new(host), server_shutdown.cancelled())
      .await
      .unwrap();
  });
  let client = AgentClient::connect(format!("http://{address}"), "message-size-test")
    .await
    .unwrap();
  (client, shutdown)
}

#[tokio::test]
async fn get_snapshot_accepts_payload_above_tonic_default() {
  let (mut client, shutdown) = connected(ABOVE_TONIC_DEFAULT).await;

  let snapshot = client.get_snapshot().await.unwrap();

  assert_eq!(
    snapshot.last_error.as_deref().map(str::len),
    Some(ABOVE_TONIC_DEFAULT)
  );
  shutdown.cancel();
}

#[tokio::test]
async fn event_stream_accepts_payload_above_tonic_default() {
  let (mut client, shutdown) = connected(ABOVE_TONIC_DEFAULT).await;
  let mut events = client.subscribe_events(0).await.unwrap();

  let event = events.message().await.unwrap().unwrap();

  assert_eq!(
    event
      .snapshot
      .and_then(|snapshot| snapshot.last_error)
      .as_deref()
      .map(str::len),
    Some(ABOVE_TONIC_DEFAULT)
  );
  shutdown.cancel();
}

#[tokio::test]
async fn get_snapshot_rejects_payload_above_protocol_limit() {
  let (mut client, shutdown) = connected(ABOVE_PROTOCOL_LIMIT).await;

  let error = client.get_snapshot().await.unwrap_err();

  assert_eq!(error.code(), tonic::Code::OutOfRange);
  assert!(error.message().contains("message length too large"));
  shutdown.cancel();
}
