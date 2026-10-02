mod support;

use std::sync::Arc;

use agent_app::{AgentCommand, RuntimeSnapshot};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::{CancelModelDownloadRequest, RequestMetadata, StartModelDownloadRequest};
use agentd::{RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use tokio::sync::{Mutex, watch};

use support::{TestServer, spawn_authenticated};

#[derive(Clone)]
struct CountingHost {
  commands: Arc<Mutex<Vec<AgentCommand>>>,
  snapshots: watch::Sender<VersionedSnapshot>,
}

impl CountingHost {
  fn new() -> Self {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(1, RuntimeSnapshot::default()));
    Self {
      commands: Arc::new(Mutex::new(Vec::new())),
      snapshots,
    }
  }
}

#[async_trait]
impl RuntimeHost for CountingHost {
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

async fn server(host: &CountingHost) -> TestServer {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  spawn_authenticated(listener, Arc::new(host.clone()))
}

fn metadata(server: &TestServer, command_id: &str) -> RequestMetadata {
  RequestMetadata {
    protocol_version: PROTOCOL_VERSION,
    client_instance_id: "model-replay-test".into(),
    command_id: command_id.into(),
    expected_server_instance_id: server.server_instance_id(),
  }
}

#[tokio::test]
async fn start_model_download_replay_executes_once_and_conflicts_on_new_payload() {
  let host = CountingHost::new();
  let server = server(&host).await;
  let mut client = server.raw_client().await;
  for _ in 0..2 {
    client
      .start_model_download(StartModelDownloadRequest {
        metadata: Some(metadata(&server, "start-download")),
        model_id: "model-a".into(),
      })
      .await
      .unwrap();
  }
  let error = client
    .start_model_download(StartModelDownloadRequest {
      metadata: Some(metadata(&server, "start-download")),
      model_id: "model-b".into(),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::AlreadyExists);
  assert_eq!(host.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn cancel_model_download_replay_executes_once_and_conflicts_on_new_payload() {
  let host = CountingHost::new();
  let server = server(&host).await;
  let mut client = server.raw_client().await;
  for _ in 0..2 {
    client
      .cancel_model_download(CancelModelDownloadRequest {
        metadata: Some(metadata(&server, "cancel-download")),
        operation_id: "operation-a".into(),
      })
      .await
      .unwrap();
  }
  let error = client
    .cancel_model_download(CancelModelDownloadRequest {
      metadata: Some(metadata(&server, "cancel-download")),
      operation_id: "operation-b".into(),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::AlreadyExists);
  assert_eq!(host.commands.lock().await.len(), 1);
}
