mod support;

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use agent_app::{
  AgentClient as _, AgentCommand, ApprovalDecision, ModelSelection, RuntimePhase, RuntimeSnapshot,
};
use agent_client::{ConnectionState, RemoteAgentClient, RemoteClientOptions};
use agent_core::ToolCallId;
use agentd::{RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use support::spawn_authenticated;

#[derive(Clone)]
struct TestHost {
  commands: Arc<Mutex<Vec<AgentCommand>>>,
  current: Arc<StdMutex<VersionedSnapshot>>,
  snapshots: Arc<StdMutex<Option<watch::Sender<VersionedSnapshot>>>>,
  subscriptions: Arc<AtomicUsize>,
}

impl TestHost {
  fn new(revision: u64, phase: RuntimePhase) -> Self {
    let initial = VersionedSnapshot::new(
      revision,
      RuntimeSnapshot {
        phase,
        ..RuntimeSnapshot::default()
      },
    );
    let (snapshots, _) = watch::channel(initial.clone());
    Self {
      commands: Arc::new(Mutex::new(Vec::new())),
      current: Arc::new(StdMutex::new(initial)),
      snapshots: Arc::new(StdMutex::new(Some(snapshots))),
      subscriptions: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn publish(&self, revision: u64, phase: RuntimePhase) {
    let snapshot = VersionedSnapshot::new(
      revision,
      RuntimeSnapshot {
        phase,
        ..RuntimeSnapshot::default()
      },
    );
    *self.current.lock().unwrap() = snapshot.clone();
    if let Some(sender) = self.snapshots.lock().unwrap().as_ref() {
      sender.send_replace(snapshot);
    }
  }

  fn close_stream_and_replace_silently(&self, revision: u64, phase: RuntimePhase) {
    self.snapshots.lock().unwrap().take();
    *self.current.lock().unwrap() = VersionedSnapshot::new(
      revision,
      RuntimeSnapshot {
        phase,
        ..RuntimeSnapshot::default()
      },
    );
  }
}

#[async_trait]
impl RuntimeHost for TestHost {
  async fn execute(&self, command: AgentCommand) -> Result<(), agent_app::RuntimeCommandError> {
    self.commands.lock().await.push(command);
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.current.lock().unwrap().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.subscriptions.fetch_add(1, Ordering::SeqCst);
    let mut sender = self.snapshots.lock().unwrap();
    if let Some(sender) = sender.as_ref() {
      return sender.subscribe();
    }
    let (replacement, receiver) = watch::channel(self.snapshot());
    *sender = Some(replacement);
    receiver
  }
}

async fn connected(host: TestHost) -> (RemoteAgentClient, CancellationToken, std::net::SocketAddr) {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let server = spawn_authenticated(listener, Arc::new(host));
  let client = RemoteAgentClient::connect(
    server.target,
    "remote-test",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  (client, server.shutdown, address)
}

#[tokio::test]
async fn connect_loads_snapshot_and_server_identity() {
  let host = TestHost::new(5, RuntimePhase::Ready);
  let (client, shutdown, _) = connected(host).await;

  assert_eq!(client.snapshot().phase, RuntimePhase::Ready);
  assert!(matches!(
    client.connection_state(),
    ConnectionState::Connected { ref server_instance_id } if !server_instance_id.is_empty()
  ));
  shutdown.cancel();
}

#[tokio::test]
async fn stream_applies_newer_complete_snapshot_with_revision_jump() {
  let host = TestHost::new(2, RuntimePhase::Ready);
  let (client, shutdown, _) = connected(host.clone()).await;
  let mut snapshots = client.subscribe();

  host.publish(40, RuntimePhase::Streaming);
  tokio::time::timeout(Duration::from_secs(1), snapshots.changed())
    .await
    .unwrap()
    .unwrap();

  assert_eq!(snapshots.borrow().phase, RuntimePhase::Streaming);
  assert_eq!(client.revision(), 40);
  shutdown.cancel();
}

#[tokio::test]
async fn consecutive_events_do_not_trigger_reconnect() {
  let host = TestHost::new(1, RuntimePhase::Ready);
  let (client, shutdown, _) = connected(host.clone()).await;
  let mut snapshots = client.subscribe();

  host.publish(2, RuntimePhase::Streaming);
  host.publish(3, RuntimePhase::Completed);
  tokio::time::timeout(Duration::from_secs(1), async {
    loop {
      if client.revision() == 3 {
        break;
      }
      snapshots.changed().await.unwrap();
    }
  })
  .await
  .unwrap();

  assert_eq!(host.subscriptions.load(Ordering::SeqCst), 1);
  assert!(matches!(
    client.connection_state(),
    ConnectionState::Connected { .. }
  ));
  shutdown.cancel();
}

#[tokio::test]
async fn execute_maps_every_application_command() {
  let host = TestHost::new(1, RuntimePhase::Ready);
  let (client, shutdown, _) = connected(host.clone()).await;
  let request_id = ToolCallId(Uuid::new_v4());
  let commands = [
    AgentCommand::StartTurn { text: "one".into() },
    AgentCommand::SteerCurrentTurn { text: "two".into() },
    AgentCommand::QueueFollowUp {
      text: "three".into(),
    },
    AgentCommand::CancelTurn,
    AgentCommand::ForceStop,
    AgentCommand::RespondApproval {
      request_id,
      decision: ApprovalDecision::DenyOnce,
    },
    AgentCommand::SelectSessionModel {
      selection: ModelSelection::Pinned {
        provider: "local".into(),
        model: "worker".into(),
      },
    },
  ];

  for command in commands.clone() {
    client.execute(command).await.unwrap();
  }

  assert_eq!(*host.commands.lock().await, commands);
  shutdown.cancel();
}

#[tokio::test]
async fn dropping_last_adapter_does_not_cancel_active_turn() {
  let host = TestHost::new(1, RuntimePhase::Streaming);
  let (client, shutdown, _) = connected(host.clone()).await;

  drop(client);
  tokio::time::sleep(Duration::from_millis(30)).await;

  assert!(host.commands.lock().await.is_empty());
  shutdown.cancel();
}

#[tokio::test]
async fn stream_disconnect_reconnects_and_resyncs_same_revision() {
  let host = TestHost::new(10, RuntimePhase::Ready);
  let (client, shutdown, _) = connected(host.clone()).await;
  let first_identity = match client.connection_state() {
    ConnectionState::Connected { server_instance_id } => server_instance_id,
    state => panic!("unexpected initial state: {state:?}"),
  };
  let mut states = client.subscribe_remote_state();

  host.close_stream_and_replace_silently(10, RuntimePhase::Completed);

  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let completed = states
        .borrow()
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.snapshot.phase == RuntimePhase::Completed);
      if completed {
        break;
      }
      states.changed().await.unwrap();
    }
  })
  .await
  .unwrap();

  let state = states.borrow().clone();
  let remote = state.snapshot.unwrap();
  assert_eq!(remote.server_instance_id, first_identity);
  assert_eq!(remote.revision, 10);
  assert!(matches!(
    state.connection,
    ConnectionState::Connected { server_instance_id } if server_instance_id == first_identity
  ));
  assert_eq!(client.revision(), 10);
  assert!(matches!(
    client.connection_state(),
    ConnectionState::Connected { server_instance_id } if server_instance_id == first_identity
  ));
  client
    .execute(AgentCommand::StartTurn {
      text: "after reconnect".into(),
    })
    .await
    .unwrap();
  assert!(matches!(
    host.commands.lock().await.as_slice(),
    [AgentCommand::StartTurn { text }] if text == "after reconnect"
  ));
  shutdown.cancel();
}
