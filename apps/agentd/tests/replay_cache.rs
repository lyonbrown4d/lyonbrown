use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentCommand, RuntimeSnapshot};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::agent_service_server::AgentService;
use agent_protocol::v1::{ForceStopRequest, RequestMetadata, StartTurnRequest};
use agentd::{AgentGrpcService, RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use tokio::sync::{Mutex, Notify, Semaphore, watch};
use tonic::Request;

#[derive(Clone)]
struct TestHost {
  commands: Arc<Mutex<Vec<AgentCommand>>>,
  snapshots: watch::Sender<VersionedSnapshot>,
  command_started: Arc<Notify>,
  slow_release: Arc<Semaphore>,
}

impl TestHost {
  fn new() -> Self {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(1, RuntimeSnapshot::default()));
    Self {
      commands: Arc::new(Mutex::new(Vec::new())),
      snapshots,
      command_started: Arc::new(Notify::new()),
      slow_release: Arc::new(Semaphore::new(0)),
    }
  }

  async fn wait_for_commands(&self, expected: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
      loop {
        let started = self.command_started.notified();
        if self.commands.lock().await.len() >= expected {
          return;
        }
        started.await;
      }
    })
    .await
    .unwrap();
  }

  fn release_slow_command(&self) {
    self.slow_release.add_permits(1);
  }
}

#[async_trait]
impl RuntimeHost for TestHost {
  async fn execute(&self, command: AgentCommand) -> Result<(), agent_app::RuntimeCommandError> {
    self.commands.lock().await.push(command.clone());
    self.command_started.notify_waiters();
    if matches!(&command, AgentCommand::StartTurn { text } if text == "slow") {
      self.slow_release.acquire().await.unwrap().forget();
    }
    if matches!(&command, AgentCommand::StartTurn { text } if text == "fail") {
      return Err(agent_app::RuntimeCommandError::EmptyText);
    }
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }
}

fn request(service: &AgentGrpcService, command_id: &str, text: &str) -> Request<StartTurnRequest> {
  Request::new(StartTurnRequest {
    metadata: Some(RequestMetadata {
      protocol_version: PROTOCOL_VERSION,
      client_instance_id: "test-client".into(),
      command_id: command_id.into(),
      expected_server_instance_id: service.server_instance_id().into(),
    }),
    text: text.into(),
  })
}

#[tokio::test]
async fn same_id_with_different_payload_is_a_conflict() {
  let host = TestHost::new();
  let service = AgentGrpcService::with_replay_capacity(Arc::new(host.clone()), 8);

  service
    .start_turn(request(&service, "command-1", "first"))
    .await
    .unwrap();
  let error = service
    .start_turn(request(&service, "command-1", "different"))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::AlreadyExists);
  assert_eq!(host.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn same_id_with_different_command_kind_is_a_conflict() {
  let host = TestHost::new();
  let service = AgentGrpcService::with_replay_capacity(Arc::new(host.clone()), 8);

  service
    .start_turn(request(&service, "command-1", "first"))
    .await
    .unwrap();
  let error = service
    .force_stop(Request::new(ForceStopRequest {
      metadata: Some(RequestMetadata {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: "test-client".into(),
        command_id: "command-1".into(),
        expected_server_instance_id: service.server_instance_id().into(),
      }),
    }))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::AlreadyExists);
  assert_eq!(host.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn replay_cache_evicts_oldest_entry_at_capacity() {
  let host = TestHost::new();
  let service = AgentGrpcService::with_replay_capacity(Arc::new(host.clone()), 2);

  for id in ["one", "two", "three", "one"] {
    service.start_turn(request(&service, id, id)).await.unwrap();
  }

  assert_eq!(host.commands.lock().await.len(), 4);
}

#[tokio::test]
async fn concurrent_retries_execute_one_logical_command_once() {
  let host = TestHost::new();
  let service = AgentGrpcService::with_replay_capacity(Arc::new(host.clone()), 8);
  let calls = (0..8).map(|_| {
    let service = service.clone();
    tokio::spawn(async move {
      service
        .start_turn(request(&service, "concurrent", "same"))
        .await
        .unwrap();
    })
  });

  for result in futures::future::join_all(calls).await {
    result.unwrap();
  }

  assert_eq!(host.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn cancelled_request_keeps_pending_command_and_replays_completion() {
  let host = TestHost::new();
  let service = AgentGrpcService::with_replay_capacity(Arc::new(host.clone()), 1);
  let first_service = service.clone();
  let first = tokio::spawn(async move {
    let request = request(&first_service, "pending", "slow");
    first_service.start_turn(request).await
  });
  host.wait_for_commands(1).await;

  first.abort();
  assert!(first.await.unwrap_err().is_cancelled());
  service
    .start_turn(request(&service, "completed", "fast"))
    .await
    .unwrap();
  let retry_service = service.clone();
  let retry = tokio::spawn(async move {
    let request = request(&retry_service, "pending", "slow");
    retry_service.start_turn(request).await
  });
  host.release_slow_command();
  retry.await.unwrap().unwrap();

  let commands = host.commands.lock().await;
  assert_eq!(
    commands
      .iter()
      .filter(|command| matches!(command, AgentCommand::StartTurn { text } if text == "slow"))
      .count(),
    1
  );
}

#[tokio::test]
async fn slow_key_does_not_block_a_different_control_command() {
  let host = TestHost::new();
  let service = AgentGrpcService::with_replay_capacity(Arc::new(host.clone()), 8);
  let slow_service = service.clone();
  let slow = tokio::spawn(async move {
    slow_service
      .start_turn(request(&slow_service, "slow-command", "slow"))
      .await
  });
  host.wait_for_commands(1).await;

  tokio::time::timeout(
    Duration::from_millis(50),
    service.force_stop(Request::new(ForceStopRequest {
      metadata: Some(RequestMetadata {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: "test-client".into(),
        command_id: "force-stop".into(),
        expected_server_instance_id: service.server_instance_id().into(),
      }),
    })),
  )
  .await
  .expect("a different command key must not wait for the slow command")
  .unwrap();

  host.release_slow_command();
  slow.await.unwrap().unwrap();
}

#[tokio::test]
async fn failed_command_result_is_replayed_without_reexecution() {
  let host = TestHost::new();
  let service = AgentGrpcService::with_replay_capacity(Arc::new(host.clone()), 8);

  let first = service
    .start_turn(request(&service, "failed", "fail"))
    .await
    .unwrap_err();
  let replay = service
    .start_turn(request(&service, "failed", "fail"))
    .await
    .unwrap_err();

  assert_eq!(first.code(), replay.code());
  assert_eq!(first.details(), replay.details());
  assert_eq!(host.commands.lock().await.len(), 1);
}
