#![allow(dead_code)]

pub mod baseline;

use std::net::SocketAddr;
use std::sync::Arc;

use agent_client::AuthenticatedTarget;
use agent_client::AuthorizationInterceptor;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint, SecretToken};
use agent_protocol::MAX_GRPC_MESSAGE_BYTES;
use agent_protocol::v1::agent_service_client::AgentServiceClient;
use agentd::{RuntimeHost, ServeError, serve_authenticated};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

const TEST_SCOPE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub fn test_scope() -> ScopeFingerprint {
  ScopeFingerprint::parse(TEST_SCOPE).expect("test scope must be canonical")
}

pub fn test_identity() -> Uuid {
  Uuid::new_v4()
}

pub struct TestServer {
  pub target: AuthenticatedTarget,
  pub shutdown: CancellationToken,
  pub task: JoinHandle<Result<(), ServeError>>,
  token: SecretToken,
  server_instance_id: Uuid,
}

pub type RawClient = AgentServiceClient<InterceptedService<Channel, AuthorizationInterceptor>>;

impl TestServer {
  pub fn server_instance_id(&self) -> String {
    self.server_instance_id.to_string()
  }

  pub async fn raw_client(&self) -> RawClient {
    let channel = Endpoint::from_shared(self.target.endpoint().to_owned())
      .expect("test endpoint")
      .connect()
      .await
      .expect("connect authenticated test client");
    AgentServiceClient::with_interceptor(channel, AuthorizationInterceptor::new(&self.token))
      .max_decoding_message_size(MAX_GRPC_MESSAGE_BYTES)
      .max_encoding_message_size(MAX_GRPC_MESSAGE_BYTES)
  }
}

#[derive(Clone)]
pub struct TestCredentials {
  descriptor: DiscoveryDescriptor,
}

impl TestCredentials {
  pub fn for_address(address: SocketAddr) -> Self {
    let descriptor = DiscoveryDescriptor::new(
      address,
      std::process::id(),
      test_identity(),
      SecretToken::generate().expect("test credential"),
      test_scope(),
    )
    .expect("test descriptor");
    Self { descriptor }
  }

  pub fn target(&self) -> AuthenticatedTarget {
    AuthenticatedTarget::from_descriptor(&self.descriptor)
  }

  pub fn spawn(self, listener: TcpListener, host: Arc<dyn RuntimeHost>) -> TestServer {
    let target = self.target();
    let token = self.descriptor.token().clone();
    let client_token = token.clone();
    let server_instance_id = self.descriptor.server_instance_id();
    let scope = self.descriptor.scope_fingerprint().clone();
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let task = tokio::spawn(async move {
      serve_authenticated(
        listener,
        host,
        token,
        server_instance_id,
        scope,
        server_shutdown.cancelled(),
      )
      .await
    });
    TestServer {
      target,
      shutdown,
      task,
      token: client_token,
      server_instance_id,
    }
  }
}

pub fn test_target(address: SocketAddr) -> AuthenticatedTarget {
  TestCredentials::for_address(address).target()
}

pub fn spawn_authenticated(listener: TcpListener, host: Arc<dyn RuntimeHost>) -> TestServer {
  let credentials = TestCredentials::for_address(listener.local_addr().expect("listener address"));
  credentials.spawn(listener, host)
}
