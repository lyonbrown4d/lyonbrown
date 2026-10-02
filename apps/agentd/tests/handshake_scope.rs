mod support;

use std::sync::Arc;

use agent_app::{AgentCommand, RuntimeCommandError, RuntimeSnapshot};
use agent_client::AgentClient;
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::{HandshakeRequest, RequestMetadata};
use agentd::{CAPABILITY_MODEL_SETUP_DOWNLOAD_V1, RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use tokio::sync::watch;

use support::{TestCredentials, spawn_authenticated};

const TEST_SCOPE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct StaticHost {
  snapshots: watch::Sender<VersionedSnapshot>,
  capabilities: &'static [&'static str],
}

impl StaticHost {
  fn new() -> Self {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(1, RuntimeSnapshot::default()));
    Self {
      snapshots,
      capabilities: &[],
    }
  }

  fn with_model_download() -> Self {
    let mut host = Self::new();
    host.capabilities = &[CAPABILITY_MODEL_SETUP_DOWNLOAD_V1];
    host
  }
}

#[async_trait]
impl RuntimeHost for StaticHost {
  async fn execute(&self, _command: AgentCommand) -> Result<(), RuntimeCommandError> {
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }

  fn capabilities(&self) -> &'static [&'static str] {
    self.capabilities
  }
}

#[tokio::test]
async fn handshake_returns_the_configured_scope_identity() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let credentials = TestCredentials::for_address(listener.local_addr().unwrap());
  let server = credentials.spawn(listener, Arc::new(StaticHost::new()));
  let server_instance_id = server.server_instance_id();

  let response = server
    .raw_client()
    .await
    .handshake(HandshakeRequest {
      metadata: Some(RequestMetadata {
        protocol_version: PROTOCOL_VERSION,
        client_instance_id: "scope-test".into(),
        command_id: "handshake-1".into(),
        expected_server_instance_id: String::new(),
      }),
      client_name: "agent-test".into(),
      client_version: "0.1.0".into(),
    })
    .await
    .unwrap()
    .into_inner();

  assert_eq!(response.scope_fingerprint, TEST_SCOPE);
  assert_eq!(response.server_instance_id, server_instance_id);
}

#[tokio::test]
async fn client_consumes_the_validated_handshake_scope() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_authenticated(listener, Arc::new(StaticHost::new()));
  let mut client = AgentClient::connect(server.target.clone(), "scope-client")
    .await
    .unwrap();

  let response = client.handshake("scope-client", "0.1.0").await.unwrap();

  assert_eq!(response.scope_fingerprint, TEST_SCOPE);
  assert_eq!(
    client
      .server_scope_fingerprint()
      .expect("scope is stored after handshake"),
    TEST_SCOPE
  );
  server.shutdown.cancel();
}

#[tokio::test]
async fn handshake_does_not_advertise_model_download_without_host_support() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_authenticated(listener, Arc::new(StaticHost::new()));
  let mut client = AgentClient::connect(server.target.clone(), "capability-client")
    .await
    .unwrap();

  let response = client
    .handshake("capability-client", "0.1.0")
    .await
    .unwrap();

  assert!(
    !response
      .capabilities
      .iter()
      .any(|value| value == CAPABILITY_MODEL_SETUP_DOWNLOAD_V1)
  );
  server.shutdown.cancel();
}

#[tokio::test]
async fn handshake_advertises_model_download_only_for_capable_host() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_authenticated(listener, Arc::new(StaticHost::with_model_download()));
  let mut client = AgentClient::connect(server.target.clone(), "capability-client")
    .await
    .unwrap();

  let response = client
    .handshake("capability-client", "0.1.0")
    .await
    .unwrap();

  assert!(
    response
      .capabilities
      .iter()
      .any(|value| value == CAPABILITY_MODEL_SETUP_DOWNLOAD_V1)
  );
  server.shutdown.cancel();
}
