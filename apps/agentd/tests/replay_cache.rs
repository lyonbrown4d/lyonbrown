mod support;

use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentCommand, RuntimeSnapshot};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::{ForceStopRequest, RequestMetadata, StartTurnRequest};
use agentd::{RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use tokio::sync::{Mutex, Notify, Semaphore, watch};

use support::{TestServer, spawn_authenticated};

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

async fn server(host: &TestHost) -> TestServer {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  spawn_authenticated(listener, Arc::new(host.clone()))
}

fn metadata(server: &TestServer, command_id: &str) -> RequestMetadata {
  RequestMetadata {
    protocol_version: PROTOCOL_VERSION,
    client_instance_id: "test-client".into(),
    command_id: command_id.into(),
    expected_server_instance_id: server.server_instance_id(),
  }
}

fn request(server: &TestServer, command_id: &str, text: &str) -> StartTurnRequest {
  StartTurnRequest {
    metadata: Some(metadata(server, command_id)),
    text: text.into(),
  }
}

#[tokio::test]
async fn same_id_with_different_payload_is_a_conflict() {
  let host = TestHost::new();
  let server = server(&host).await;
  let mut client = server.raw_client().await;
  client
    .start_turn(request(&server, "command-1", "first"))
    .await
    .unwrap();
  let error = client
    .start_turn(request(&server, "command-1", "different"))
    .await
    .unwrap_err();
  assert_eq!(error.code(), tonic::Code::AlreadyExists);
  assert_eq!(host.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn same_id_with_different_command_kind_is_a_conflict() {
  let host = TestHost::new();
  let server = server(&host).await;
  let mut client = server.raw_client().await;
  client
    .start_turn(request(&server, "command-1", "first"))
    .await
    .unwrap();
  let error = client
    .force_stop(ForceStopRequest {
      metadata: Some(metadata(&server, "command-1")),
    })
    .await
    .unwrap_err();
  assert_eq!(error.code(), tonic::Code::AlreadyExists);
  assert_eq!(host.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn replay_cache_evicts_oldest_entry_at_capacity() {
  let host = TestHost::new();
  let server = server(&host).await;
  let mut client = server.raw_client().await;
  for index in 0..=1024 {
    let id = format!("command-{index}");
    client.start_turn(request(&server, &id, &id)).await.unwrap();
  }
  client
    .start_turn(request(&server, "command-0", "command-0"))
    .await
    .unwrap();
  assert_eq!(host.commands.lock().await.len(), 1026);
}

#[tokio::test]
async fn concurrent_retries_execute_one_logical_command_once() {
  let host = TestHost::new();
  let server = Arc::new(server(&host).await);
  let mut calls = Vec::new();
  for _ in 0..8 {
    let mut client = server.raw_client().await;
    let server = Arc::clone(&server);
    calls.push(tokio::spawn(async move {
      client
        .start_turn(request(&server, "concurrent", "same"))
        .await
        .unwrap();
    }));
  }
  for result in futures::future::join_all(calls).await {
    result.unwrap();
  }
  assert_eq!(host.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn cancelled_request_keeps_pending_command_and_replays_completion() {
  let host = TestHost::new();
  let server = Arc::new(server(&host).await);
  let mut first_client = server.raw_client().await;
  let first_server = Arc::clone(&server);
  let first = tokio::spawn(async move {
    first_client
      .start_turn(request(&first_server, "pending", "slow"))
      .await
  });
  host.wait_for_commands(1).await;
  first.abort();
  assert!(first.await.unwrap_err().is_cancelled());
  let mut client = server.raw_client().await;
  client
    .start_turn(request(&server, "completed", "fast"))
    .await
    .unwrap();
  let retry_server = Arc::clone(&server);
  let retry = tokio::spawn(async move {
    client
      .start_turn(request(&retry_server, "pending", "slow"))
      .await
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
  let server = Arc::new(server(&host).await);
  let mut slow_client = server.raw_client().await;
  let slow_server = Arc::clone(&server);
  let slow = tokio::spawn(async move {
    slow_client
      .start_turn(request(&slow_server, "slow-command", "slow"))
      .await
  });
  host.wait_for_commands(1).await;
  let mut client = server.raw_client().await;
  tokio::time::timeout(
    Duration::from_millis(100),
    client.force_stop(ForceStopRequest {
      metadata: Some(metadata(&server, "force-stop")),
    }),
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
  let server = server(&host).await;
  let mut client = server.raw_client().await;
  let first = client
    .start_turn(request(&server, "failed", "fail"))
    .await
    .unwrap_err();
  let replay = client
    .start_turn(request(&server, "failed", "fail"))
    .await
    .unwrap_err();
  assert_eq!(first.code(), replay.code());
  assert_eq!(first.details(), replay.details());
  assert_eq!(host.commands.lock().await.len(), 1);
}
