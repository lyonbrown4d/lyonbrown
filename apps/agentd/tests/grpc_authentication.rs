mod support;

use std::sync::Arc;
use std::time::Duration;

use agent_app::{AgentClientError, ProtocolErrorKind, RuntimeSnapshot};
use agent_client::{
  AgentClient, AuthenticatedTarget, ConnectionState, NativeAgentClient, RemoteClientOptions,
};
use agent_local::{
  AUTHORIZATION_METADATA_KEY, DiscoveryDescriptor, SecretToken, bearer_authorization,
};
use agent_protocol::v1::agent_service_client::AgentServiceClient as RawAgentClient;
use agent_protocol::v1::{
  HealthRequest, StartModelDownloadRequest, StartTurnRequest, SubscribeEventsRequest,
};
use agentd::{RuntimeHost, VersionedSnapshot, serve_authenticated};
use async_trait::async_trait;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::{Code, Request};

use support::{test_identity, test_scope};

#[derive(Clone)]
struct TestHost {
  snapshots: watch::Sender<VersionedSnapshot>,
}

impl TestHost {
  fn new() -> Self {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(1, RuntimeSnapshot::default()));
    Self { snapshots }
  }
}

#[async_trait]
impl RuntimeHost for TestHost {
  async fn execute(
    &self,
    _command: agent_app::AgentCommand,
  ) -> Result<(), agent_app::RuntimeCommandError> {
    Ok(())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }
}

async fn server() -> (DiscoveryDescriptor, CancellationToken) {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let token = SecretToken::generate().expect("token");
  let identity = test_identity();
  let scope = test_scope();
  let descriptor = DiscoveryDescriptor::new(
    address,
    std::process::id(),
    identity,
    token.clone(),
    scope.clone(),
  )
  .expect("validated discovery descriptor");
  let shutdown = CancellationToken::new();
  let server_shutdown = shutdown.clone();
  tokio::spawn(async move {
    serve_authenticated(
      listener,
      Arc::new(TestHost::new()),
      token,
      identity,
      scope,
      server_shutdown.cancelled(),
    )
    .await
    .unwrap();
  });
  (descriptor, shutdown)
}

#[tokio::test]
async fn authenticated_server_rejects_invalid_credentials_uniformly() {
  let (descriptor, shutdown) = server().await;
  let endpoint = format!("http://{}", descriptor.address());
  let token = descriptor.token();
  let mut client = RawAgentClient::connect(endpoint).await.unwrap();
  let wrong = SecretToken::generate().expect("wrong token");
  let valid: MetadataValue<Ascii> = bearer_authorization(token).parse().unwrap();
  let cases = [None, Some("Basic invalid"), Some("Bearer invalid")];
  for authorization in cases {
    let mut request = Request::new(HealthRequest { metadata: None });
    if let Some(authorization) = authorization {
      request
        .metadata_mut()
        .insert(AUTHORIZATION_METADATA_KEY, authorization.parse().unwrap());
    }
    let error = client.health(request).await.unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    assert_eq!(error.message(), "authentication required");
  }
  let mut wrong_request = Request::new(HealthRequest { metadata: None });
  wrong_request.metadata_mut().insert(
    AUTHORIZATION_METADATA_KEY,
    bearer_authorization(&wrong).parse().unwrap(),
  );
  let error = client.health(wrong_request).await.unwrap_err();
  assert_eq!(error.code(), Code::Unauthenticated);
  assert_eq!(error.message(), "authentication required");

  let mut duplicate = Request::new(HealthRequest { metadata: None });
  duplicate
    .metadata_mut()
    .append(AUTHORIZATION_METADATA_KEY, valid.clone());
  duplicate
    .metadata_mut()
    .append(AUTHORIZATION_METADATA_KEY, valid);
  let error = client.health(duplicate).await.unwrap_err();
  assert_eq!(error.code(), Code::Unauthenticated);
  assert_eq!(error.message(), "authentication required");

  let error = client
    .start_model_download(StartModelDownloadRequest {
      metadata: None,
      model_id: "qwen3-1.7b-q4_k_m".into(),
    })
    .await
    .expect_err("model download without authentication must fail");
  assert_eq!(error.code(), Code::Unauthenticated);
  assert_eq!(error.message(), "authentication required");

  let error = client
    .start_turn(StartTurnRequest {
      metadata: None,
      text: "must not execute".into(),
    })
    .await
    .expect_err("command without authentication must fail");
  assert_eq!(error.code(), Code::Unauthenticated);
  assert_eq!(error.message(), "authentication required");

  let error = client
    .subscribe_events(SubscribeEventsRequest {
      metadata: None,
      after_revision: 0,
    })
    .await
    .expect_err("stream without authentication must fail");
  assert_eq!(error.code(), Code::Unauthenticated);
  assert_eq!(error.message(), "authentication required");
  shutdown.cancel();
}

#[tokio::test]
async fn authenticated_client_can_call_unary_and_streaming_rpcs() {
  let (descriptor, shutdown) = server().await;
  let target = AuthenticatedTarget::from_descriptor(&descriptor);
  let mut client = AgentClient::connect(target, "auth-test").await.unwrap();

  client.health().await.unwrap();
  client.handshake("auth-test", "0.1.0").await.unwrap();
  let snapshot = client.get_snapshot().await.unwrap();
  client.subscribe_events(snapshot.revision).await.unwrap();

  shutdown.cancel();
}

#[tokio::test]
async fn wrong_credential_maps_to_terminal_authentication_failure() {
  let (descriptor, shutdown) = server().await;
  let wrong = SecretToken::generate().expect("wrong token");
  let wrong_descriptor = DiscoveryDescriptor::new(
    descriptor.address(),
    descriptor.pid(),
    descriptor.server_instance_id(),
    wrong,
    descriptor.scope_fingerprint().clone(),
  )
  .expect("validated discovery descriptor");
  let client = NativeAgentClient::start(
    AuthenticatedTarget::from_descriptor(&wrong_descriptor),
    "wrong-auth-test",
    RemoteClientOptions::for_tests(),
  )
  .unwrap();
  let mut states = client.subscribe_remote_state();

  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if matches!(
        &states.borrow().connection,
        ConnectionState::Failed {
          error: AgentClientError::Protocol {
            kind: ProtocolErrorKind::AuthenticationFailed,
            ..
          },
          ..
        }
      ) {
        break;
      }
      states.changed().await.unwrap();
    }
  })
  .await
  .expect("authentication failure must be terminal");
  tokio::time::sleep(Duration::from_millis(100)).await;
  assert!(!client.driver_monitor().is_running());

  shutdown.cancel();
}
