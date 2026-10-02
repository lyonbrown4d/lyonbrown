mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_app::{
  AgentClient as _, AgentClientError, AgentCommand, AvailabilityKind, RuntimePhase, RuntimeSnapshot,
};
use agent_client::{ConnectionState, NativeAgentClient, NativeDriverMonitor, RemoteClientOptions};
use agentd::{RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify, watch};

use support::{TestCredentials, TestServer, spawn_authenticated};

#[derive(Clone)]
struct TestHost {
  commands: Arc<Mutex<Vec<AgentCommand>>>,
  snapshots: watch::Sender<VersionedSnapshot>,
  command_gate: Option<Arc<Notify>>,
}

impl TestHost {
  fn new(phase: RuntimePhase) -> Self {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(
      7,
      RuntimeSnapshot {
        phase,
        ..RuntimeSnapshot::default()
      },
    ));
    Self {
      commands: Arc::new(Mutex::new(Vec::new())),
      snapshots,
      command_gate: None,
    }
  }

  fn blocking() -> Self {
    Self {
      command_gate: Some(Arc::new(Notify::new())),
      ..Self::new(RuntimePhase::Streaming)
    }
  }
}

#[async_trait]
impl RuntimeHost for TestHost {
  async fn execute(&self, command: AgentCommand) -> Result<(), agent_app::RuntimeCommandError> {
    self.commands.lock().await.push(command);
    if let Some(gate) = &self.command_gate {
      gate.notified().await;
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

fn spawn_server(listener: TcpListener, host: TestHost) -> TestServer {
  spawn_authenticated(listener, Arc::new(host))
}

async fn wait_connected(client: &NativeAgentClient) {
  let mut states = client.subscribe_remote_state();
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if matches!(
        states.borrow().connection,
        ConnectionState::Connected { .. }
      ) {
        break;
      }
      states.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
}

fn wait_stopped(monitor: &NativeDriverMonitor) {
  let deadline = Instant::now() + Duration::from_secs(2);
  while monitor.is_running() && Instant::now() < deadline {
    std::thread::sleep(Duration::from_millis(5));
  }
  assert!(!monitor.is_running());
}

#[test]
fn starts_without_an_external_tokio_runtime() {
  let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let address = reserved.local_addr().unwrap();
  let credentials = TestCredentials::for_address(address);
  drop(reserved);
  let client = NativeAgentClient::start(
    credentials.target(),
    "native-no-runtime",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  let monitor = client.driver_monitor();

  let error = futures::executor::block_on(client.dispatch(AgentCommand::StartTurn {
    text: "offline".into(),
  }))
  .unwrap_err();
  assert!(matches!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::Disconnected,
      ..
    }
  ));

  drop(client);
  wait_stopped(&monitor);
}

#[tokio::test]
async fn connects_publishes_snapshot_and_dispatches_command() {
  let host = TestHost::new(RuntimePhase::Ready);
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_server(listener, host.clone());
  let client = NativeAgentClient::start(
    server.target,
    "native-connected",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();

  wait_connected(&client).await;
  assert_eq!(client.snapshot().phase, RuntimePhase::Ready);
  client
    .execute(AgentCommand::StartTurn {
      text: "hello".into(),
    })
    .await
    .unwrap();
  assert!(matches!(
    host.commands.lock().await.as_slice(),
    [AgentCommand::StartTurn { text }] if text == "hello"
  ));

  drop(client);
  server.shutdown.cancel();
  server.task.abort();
}

#[tokio::test]
async fn retries_initial_transport_failure_until_server_appears() {
  let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = reserved.local_addr().unwrap();
  let credentials = TestCredentials::for_address(address);
  drop(reserved);
  let client = NativeAgentClient::start(
    credentials.target(),
    "native-late-server",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  tokio::time::sleep(Duration::from_millis(30)).await;

  let listener = TcpListener::bind(address).await.unwrap();
  let server = credentials.spawn(listener, Arc::new(TestHost::new(RuntimePhase::Ready)));
  wait_connected(&client).await;

  drop(client);
  server.shutdown.cancel();
  server.task.abort();
}

#[tokio::test]
async fn last_drop_stops_thread_resolves_pending_and_never_cancels_turn() {
  let host = TestHost::blocking();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_server(listener, host.clone());
  let client = NativeAgentClient::start(
    server.target,
    "native-drop",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  wait_connected(&client).await;
  let monitor = client.driver_monitor();
  let pending = tokio::spawn(client.dispatch(AgentCommand::StartTurn {
    text: "pending".into(),
  }));
  tokio::time::timeout(Duration::from_secs(1), async {
    while host.commands.lock().await.is_empty() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();

  drop(client);
  let error = tokio::time::timeout(Duration::from_secs(1), pending)
    .await
    .unwrap()
    .unwrap()
    .unwrap_err();
  assert!(matches!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::ShuttingDown,
      ..
    }
  ));
  wait_stopped(&monitor);
  assert!(matches!(
    host.commands.lock().await.as_slice(),
    [AgentCommand::StartTurn { .. }]
  ));

  server.shutdown.cancel();
  server.task.abort();
}
