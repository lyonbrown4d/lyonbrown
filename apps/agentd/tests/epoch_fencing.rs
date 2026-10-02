mod support;

use std::sync::Arc;

use agent_app::{AgentCommand, RuntimeSnapshot};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::command_error_detail::Context;
use agent_protocol::v1::{
  CommandErrorCode, CommandErrorDetail, EpochMismatchDetail, RequestMetadata,
  StartModelDownloadRequest, StartTurnRequest,
};
use agentd::{RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use prost::Message;
use tokio::sync::{Mutex, watch};

use support::spawn_authenticated;

#[derive(Clone)]
struct RecordingHost {
  commands: Arc<Mutex<Vec<AgentCommand>>>,
  snapshots: watch::Sender<VersionedSnapshot>,
}

impl RecordingHost {
  fn new() -> Self {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(1, RuntimeSnapshot::default()));
    Self {
      commands: Arc::new(Mutex::new(Vec::new())),
      snapshots,
    }
  }
}

#[async_trait]
impl RuntimeHost for RecordingHost {
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

fn command(expected_server_instance_id: &str) -> StartTurnRequest {
  StartTurnRequest {
    metadata: Some(RequestMetadata {
      protocol_version: PROTOCOL_VERSION,
      client_instance_id: "epoch-test".into(),
      command_id: "logical-command".into(),
      expected_server_instance_id: expected_server_instance_id.into(),
    }),
    text: "do this once".into(),
  }
}

#[tokio::test]
async fn command_from_an_old_daemon_epoch_is_not_executed_by_the_new_daemon() {
  let old_host = RecordingHost::new();
  let old_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let old_server = spawn_authenticated(old_listener, Arc::new(old_host.clone()));
  let old_epoch = old_server.server_instance_id();
  old_server
    .raw_client()
    .await
    .start_turn(command(&old_epoch))
    .await
    .unwrap();

  let new_host = RecordingHost::new();
  let new_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let new_server = spawn_authenticated(new_listener, Arc::new(new_host.clone()));
  assert_ne!(old_epoch, new_server.server_instance_id());
  let error = new_server
    .raw_client()
    .await
    .start_turn(command(&old_epoch))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::FailedPrecondition);
  let detail = CommandErrorDetail::decode(error.details()).unwrap();
  assert_eq!(detail.code, CommandErrorCode::EpochMismatch as i32);
  assert_eq!(
    detail.context,
    Some(Context::EpochMismatch(EpochMismatchDetail {
      expected_server_instance_id: old_epoch.clone(),
      actual_server_instance_id: new_server.server_instance_id(),
    }))
  );
  assert_eq!(old_host.commands.lock().await.len(), 1);
  assert!(new_host.commands.lock().await.is_empty());
}

#[tokio::test]
async fn command_without_a_handshake_epoch_is_rejected() {
  let host = RecordingHost::new();
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_authenticated(listener, Arc::new(host.clone()));

  let error = server
    .raw_client()
    .await
    .start_turn(command(""))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::FailedPrecondition);
  let detail = CommandErrorDetail::decode(error.details()).unwrap();
  assert_eq!(detail.code, CommandErrorCode::EpochMismatch as i32);
  assert!(host.commands.lock().await.is_empty());
}

#[tokio::test]
async fn model_download_from_an_old_daemon_epoch_is_rejected() {
  let host = RecordingHost::new();
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_authenticated(listener, Arc::new(host.clone()));

  let error = server
    .raw_client()
    .await
    .start_model_download(StartModelDownloadRequest {
      metadata: Some(RequestMetadata {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: "epoch-test".into(),
        command_id: "download-command".into(),
        expected_server_instance_id: "old-daemon".into(),
      }),
      model_id: "qwen3-1.7b-q4_k_m".into(),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::FailedPrecondition);
  assert!(host.commands.lock().await.is_empty());
}
