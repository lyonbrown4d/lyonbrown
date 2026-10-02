mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_app::{AgentClient as _, AgentCommand, ProtocolErrorKind};
use agent_client::{
  AuthenticatedTarget, AuthenticatedTargetResolver, ConnectionState, RemoteAgentClient,
  RemoteClientOptions,
};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::RuntimePhase;
use async_trait::async_trait;
use tokio::sync::RwLock;

use support::reconnect_server::TestEpoch;

struct SwitchingResolver {
  target: RwLock<AuthenticatedTarget>,
  calls: AtomicUsize,
}

impl SwitchingResolver {
  fn new(target: AuthenticatedTarget) -> Self {
    Self {
      target: RwLock::new(target),
      calls: AtomicUsize::new(0),
    }
  }

  async fn switch_to(&self, target: AuthenticatedTarget) {
    *self.target.write().await = target;
  }
}

#[async_trait]
impl AuthenticatedTargetResolver for SwitchingResolver {
  async fn resolve(&self) -> Result<AuthenticatedTarget, agent_client::AgentClientError> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    Ok(self.target.read().await.clone())
  }
}

#[tokio::test]
async fn daemon_epoch_change_re_resolves_events_and_commands() {
  let epoch_a = TestEpoch::spawn(RuntimePhase::Ready, PROTOCOL_VERSION).await;
  let resolver = Arc::new(SwitchingResolver::new(epoch_a.target.clone()));
  let client = RemoteAgentClient::connect_with_resolver(
    resolver.clone(),
    "restart-test",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  let epoch_b = TestEpoch::spawn(RuntimePhase::Completed, PROTOCOL_VERSION).await;
  resolver.switch_to(epoch_b.target.clone()).await;
  let mut state = client.subscribe_remote_state();

  epoch_a.stream_shutdown.cancel();

  tokio::time::timeout(std::time::Duration::from_secs(2), async {
    loop {
      let connected_to_b = matches!(
        &state.borrow().connection,
        ConnectionState::Connected { server_instance_id }
          if server_instance_id == &epoch_b.server_instance_id
      );
      if connected_to_b {
        break;
      }
      state.changed().await.unwrap();
    }
  })
  .await
  .unwrap();

  client
    .execute(AgentCommand::StartTurn {
      text: "new epoch command".into(),
    })
    .await
    .unwrap();
  assert_eq!(
    epoch_b.commands.lock().unwrap().as_slice(),
    ["new epoch command"]
  );
  assert!(epoch_a.commands.lock().unwrap().is_empty());
  assert!(resolver.calls.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn protocol_error_after_restart_is_terminal() {
  let epoch_a = TestEpoch::spawn(RuntimePhase::Ready, PROTOCOL_VERSION).await;
  let resolver = Arc::new(SwitchingResolver::new(epoch_a.target.clone()));
  let client = RemoteAgentClient::connect_with_resolver(
    resolver.clone(),
    "protocol-test",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  let incompatible = TestEpoch::spawn(RuntimePhase::Ready, PROTOCOL_VERSION + 1).await;
  resolver.switch_to(incompatible.target.clone()).await;
  let mut state = client.subscribe_remote_state();

  epoch_a.stream_shutdown.cancel();

  tokio::time::timeout(std::time::Duration::from_secs(2), async {
    loop {
      if matches!(
        &state.borrow().connection,
        ConnectionState::Failed {
          error: agent_client::AgentClientError::Protocol {
            kind: ProtocolErrorKind::IncompatibleVersion,
            ..
          },
          ..
        }
      ) {
        break;
      }
      state.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
  let calls = resolver.calls.load(Ordering::SeqCst);
  tokio::time::sleep(std::time::Duration::from_millis(75)).await;
  assert_eq!(resolver.calls.load(Ordering::SeqCst), calls);
}
