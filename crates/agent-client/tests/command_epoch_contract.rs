#[path = "support/command_server.rs"]
mod command_server;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use agent_app::{AgentClient as _, AgentClientError, AgentCommand, AvailabilityKind};
use agent_client::{
  AuthenticatedTarget, AuthenticatedTargetResolver, ConnectionState, NativeAgentClient,
  RemoteAgentClient, RemoteClientOptions,
};
use async_trait::async_trait;
use tokio::sync::RwLock;

use command_server::{CommandServer, ReceiptMode};

struct SwitchingResolver {
  target: RwLock<AuthenticatedTarget>,
}

impl SwitchingResolver {
  fn new(target: AuthenticatedTarget) -> Self {
    Self {
      target: RwLock::new(target),
    }
  }

  async fn switch_to(&self, target: AuthenticatedTarget) {
    *self.target.write().await = target;
  }
}

#[async_trait]
impl AuthenticatedTargetResolver for SwitchingResolver {
  async fn resolve(&self) -> Result<AuthenticatedTarget, AgentClientError> {
    Ok(self.target.read().await.clone())
  }
}

#[tokio::test]
async fn queued_epoch_a_command_is_not_delivered_to_epoch_b() {
  let epoch_a = CommandServer::spawn(ReceiptMode::Valid, true).await;
  let resolver = Arc::new(SwitchingResolver::new(epoch_a.target.clone()));
  let client = NativeAgentClient::start_with_resolver(
    resolver.clone(),
    "native-epoch-test",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  wait_for_epoch(&client, &epoch_a.server_instance_id).await;

  let blocker = client.dispatch_for_server(
    epoch_a.server_instance_id.clone(),
    AgentCommand::StartTurn {
      text: "blocking-a".into(),
    },
  );
  wait_for_commands(&epoch_a, 1).await;
  let stale = client.dispatch_for_server(
    epoch_a.server_instance_id.clone(),
    AgentCommand::StartTurn {
      text: "must-not-reach-b".into(),
    },
  );

  let epoch_b = CommandServer::spawn(ReceiptMode::Valid, false).await;
  resolver.switch_to(epoch_b.target.clone()).await;
  epoch_a.stream_shutdown.cancel();
  wait_for_handshake_and_subscription(&epoch_b).await;
  epoch_a.start_gate.as_ref().unwrap().add_permits(1);

  blocker.await.unwrap();
  let error = stale.await.unwrap_err();
  assert!(matches!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::StaleEpoch | AvailabilityKind::Disconnected,
      ..
    }
  ));
  assert_eq!(epoch_a.commands.lock().unwrap().as_slice(), ["blocking-a"]);
  assert!(epoch_b.commands.lock().unwrap().is_empty());
}

#[tokio::test]
async fn remote_rejects_epoch_a_command_after_observing_connected_epoch_b() {
  let epoch_a = CommandServer::spawn(ReceiptMode::Valid, false).await;
  let resolver = Arc::new(SwitchingResolver::new(epoch_a.target.clone()));
  let client = RemoteAgentClient::connect_with_resolver(
    resolver.clone(),
    "remote-epoch-test",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  let mut state = client.subscribe_remote_state();
  let epoch_b = CommandServer::spawn(ReceiptMode::Valid, false).await;

  resolver.switch_to(epoch_b.target.clone()).await;
  epoch_a.stream_shutdown.cancel();
  wait_for_remote_epoch(&mut state, &epoch_b.server_instance_id).await;

  let error = client
    .execute_for_server(
      &epoch_a.server_instance_id,
      AgentCommand::StartTurn {
        text: "must-not-reach-b".into(),
      },
    )
    .await
    .unwrap_err();
  assert!(matches!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::StaleEpoch,
      ..
    }
  ));
  assert!(epoch_b.commands.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejected_receipt_is_a_protocol_error() {
  assert_receipt_error(ReceiptMode::Rejected, "not accepted").await;
}

#[tokio::test]
async fn mismatched_command_id_receipt_is_a_protocol_error() {
  assert_receipt_error(ReceiptMode::WrongCommandId, "command id").await;
}

#[tokio::test]
async fn mismatched_protocol_receipt_is_a_protocol_error() {
  assert_receipt_error(ReceiptMode::WrongProtocol, "protocol version").await;
}

async fn assert_receipt_error(mode: ReceiptMode, message: &str) {
  let server = CommandServer::spawn(mode, false).await;
  let client = RemoteAgentClient::connect(
    server.target.clone(),
    "receipt-test",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  let error = client
    .execute(AgentCommand::StartTurn {
      text: "test".into(),
    })
    .await
    .unwrap_err();
  assert!(matches!(
    error,
    AgentClientError::Protocol {
      kind: agent_client::ProtocolErrorKind::InvalidResponse,
      ..
    }
  ));
  assert!(error.to_string().contains(message), "{error}");
}

async fn wait_for_epoch(client: &NativeAgentClient, expected: &str) {
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if matches!(
        client.remote_state().connection,
        ConnectionState::Connected { server_instance_id } if server_instance_id == expected
      ) {
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

async fn wait_for_commands(server: &CommandServer, count: usize) {
  tokio::time::timeout(Duration::from_secs(2), async {
    while server.commands.lock().unwrap().len() < count {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

async fn wait_for_handshake_and_subscription(server: &CommandServer) {
  tokio::time::timeout(Duration::from_secs(2), async {
    while server.handshakes.load(Ordering::SeqCst) == 0
      || server.subscriptions.load(Ordering::SeqCst) == 0
    {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

async fn wait_for_remote_epoch(
  state: &mut tokio::sync::watch::Receiver<agent_client::RemoteState>,
  expected: &str,
) {
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if matches!(
        &state.borrow().connection,
        ConnectionState::Connected { server_instance_id } if server_instance_id == expected
      ) {
        break;
      }
      state.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
}
