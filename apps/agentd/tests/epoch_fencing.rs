use std::sync::Arc;

use agent_app::{AgentCommand, RuntimeSnapshot};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::agent_service_server::AgentService;
use agent_protocol::v1::{RequestMetadata, StartTurnRequest};
use agentd::{AgentGrpcService, RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use tokio::sync::{Mutex, watch};
use tonic::Request;

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

fn command(expected_server_instance_id: &str) -> Request<StartTurnRequest> {
  Request::new(StartTurnRequest {
    metadata: Some(RequestMetadata {
      protocol_version: PROTOCOL_VERSION,
      client_instance_id: "epoch-test".into(),
      command_id: "logical-command".into(),
      expected_server_instance_id: expected_server_instance_id.into(),
    }),
    text: "do this once".into(),
  })
}

#[tokio::test]
async fn command_from_an_old_daemon_epoch_is_not_executed_by_the_new_daemon() {
  let old_host = RecordingHost::new();
  let old_service = AgentGrpcService::new(Arc::new(old_host.clone()));
  let old_epoch = old_service.server_instance_id().to_owned();
  old_service.start_turn(command(&old_epoch)).await.unwrap();

  let new_host = RecordingHost::new();
  let new_service = AgentGrpcService::new(Arc::new(new_host.clone()));
  assert_ne!(old_epoch, new_service.server_instance_id());
  let error = new_service
    .start_turn(command(&old_epoch))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::FailedPrecondition);
  assert!(error.message().contains("server instance changed"));
  assert_eq!(old_host.commands.lock().await.len(), 1);
  assert!(new_host.commands.lock().await.is_empty());
}

#[tokio::test]
async fn command_without_a_handshake_epoch_is_rejected() {
  let host = RecordingHost::new();
  let service = AgentGrpcService::new(Arc::new(host.clone()));

  let error = service.start_turn(command("")).await.unwrap_err();

  assert_eq!(error.code(), tonic::Code::FailedPrecondition);
  assert!(host.commands.lock().await.is_empty());
}
