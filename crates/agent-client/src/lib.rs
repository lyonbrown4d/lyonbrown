//! Reusable tonic client for TUI and GUI frontends.

pub use agent_app::{AgentClientError, AvailabilityKind, ProtocolErrorKind};

mod auth;
mod command;
mod native;
mod remote;
mod resolver;
mod snapshot;

pub use auth::{AuthenticatedTarget, AuthorizationInterceptor};
pub use command::{ApprovalDecision, Command};
pub use native::{NativeAgentClient, NativeCommandFuture, NativeDriverMonitor};
pub use remote::{
  ConnectionState, RemoteAgentClient, RemoteClientOptions, RemoteState, VersionedRemoteSnapshot,
};
pub use resolver::AuthenticatedTargetResolver;
pub use snapshot::{DecodedSnapshot, SnapshotDecodeError, decode_snapshot};

use agent_protocol::v1::agent_service_client::AgentServiceClient;
use agent_protocol::v1::{
  GetSnapshotRequest, HandshakeRequest, HandshakeResponse, HealthRequest, HealthResponse,
  RequestMetadata, RuntimeEvent, RuntimeSnapshot, SubscribeEventsRequest,
};
use agent_protocol::{MAX_GRPC_MESSAGE_BYTES, PROTOCOL_VERSION};
use tonic::Streaming;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AgentClient {
  inner: AgentServiceClient<InterceptedService<Channel, AuthorizationInterceptor>>,
  client_instance_id: String,
  server_instance_id: Option<String>,
  server_scope_fingerprint: Option<String>,
}

impl AgentClient {
  pub async fn connect(
    target: AuthenticatedTarget,
    client_instance_id: impl Into<String>,
  ) -> Result<Self, tonic::transport::Error> {
    Self::connect_target(target, client_instance_id.into(), None).await
  }

  async fn connect_target(
    target: AuthenticatedTarget,
    client_instance_id: String,
    timeouts: Option<(std::time::Duration, std::time::Duration)>,
  ) -> Result<Self, tonic::transport::Error> {
    let (endpoint, interceptor) = target.into_parts();
    let mut endpoint = Endpoint::from_shared(endpoint)?;
    if let Some((connect_timeout, rpc_timeout)) = timeouts {
      endpoint = endpoint
        .connect_timeout(connect_timeout)
        .timeout(rpc_timeout);
    }
    let channel = endpoint.connect().await?;
    let inner = AgentServiceClient::with_interceptor(channel, interceptor)
      .max_decoding_message_size(MAX_GRPC_MESSAGE_BYTES)
      .max_encoding_message_size(MAX_GRPC_MESSAGE_BYTES);
    Ok(Self {
      inner,
      client_instance_id,
      server_instance_id: None,
      server_scope_fingerprint: None,
    })
  }

  pub(crate) async fn connect_with_timeouts(
    target: AuthenticatedTarget,
    client_instance_id: String,
    connect_timeout: std::time::Duration,
    rpc_timeout: std::time::Duration,
  ) -> Result<Self, tonic::transport::Error> {
    Self::connect_target(
      target,
      client_instance_id,
      Some((connect_timeout, rpc_timeout)),
    )
    .await
  }

  pub async fn health(&mut self) -> Result<HealthResponse, tonic::Status> {
    let metadata = Some(self.metadata());
    Ok(
      self
        .inner
        .health(HealthRequest { metadata })
        .await?
        .into_inner(),
    )
  }

  pub async fn handshake(
    &mut self,
    client_name: impl Into<String>,
    client_version: impl Into<String>,
  ) -> Result<HandshakeResponse, tonic::Status> {
    let metadata = Some(self.metadata());
    let response = self
      .inner
      .handshake(HandshakeRequest {
        metadata,
        client_name: client_name.into(),
        client_version: client_version.into(),
      })
      .await?
      .into_inner();
    self.server_instance_id = Some(response.server_instance_id.clone());
    self.server_scope_fingerprint = Some(response.scope_fingerprint.clone());
    Ok(response)
  }

  pub fn server_scope_fingerprint(&self) -> Option<&str> {
    self.server_scope_fingerprint.as_deref()
  }

  pub async fn get_snapshot(&mut self) -> Result<RuntimeSnapshot, tonic::Status> {
    let metadata = Some(self.metadata());
    Ok(
      self
        .inner
        .get_snapshot(GetSnapshotRequest { metadata })
        .await?
        .into_inner(),
    )
  }

  pub async fn subscribe_events(
    &mut self,
    after_revision: u64,
  ) -> Result<Streaming<RuntimeEvent>, tonic::Status> {
    let metadata = Some(self.metadata());
    Ok(
      self
        .inner
        .subscribe_events(SubscribeEventsRequest {
          metadata,
          after_revision,
        })
        .await?
        .into_inner(),
    )
  }

  fn metadata(&self) -> RequestMetadata {
    self.metadata_with_id(Uuid::new_v4().to_string())
  }

  fn metadata_with_id(&self, command_id: String) -> RequestMetadata {
    RequestMetadata {
      protocol_version: PROTOCOL_VERSION,
      client_instance_id: self.client_instance_id.clone(),
      command_id,
      expected_server_instance_id: self.server_instance_id.clone().unwrap_or_default(),
    }
  }

  fn metadata_with_id_for_server(
    &self,
    command_id: String,
    server_instance_id: String,
  ) -> RequestMetadata {
    RequestMetadata {
      expected_server_instance_id: server_instance_id,
      ..self.metadata_with_id(command_id)
    }
  }
}
