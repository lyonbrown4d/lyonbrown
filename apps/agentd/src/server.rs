use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use agent_local::{ScopeFingerprint, SecretToken};
use agent_protocol::{MAX_GRPC_MESSAGE_BYTES, v1::agent_service_server::AgentServiceServer};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::RuntimeHost;
use crate::auth::ServerAuthorization;
use crate::service::AgentGrpcService;

#[derive(Debug)]
pub enum ServeError {
  Io(std::io::Error),
  Transport(tonic::transport::Error),
  NonLoopback(SocketAddr),
}

impl std::fmt::Display for ServeError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Io(error) => write!(formatter, "failed to inspect listener: {error}"),
      Self::Transport(error) => write!(formatter, "gRPC server failed: {error}"),
      Self::NonLoopback(address) => {
        write!(
          formatter,
          "agentd v1 only accepts loopback listeners, got {address}"
        )
      }
    }
  }
}

impl std::error::Error for ServeError {}

/// Product server path. Every RPC is protected by one per-launch bearer credential.
pub async fn serve_authenticated<F>(
  listener: TcpListener,
  host: Arc<dyn RuntimeHost>,
  token: SecretToken,
  server_instance_id: Uuid,
  scope_fingerprint: ScopeFingerprint,
  shutdown: F,
) -> Result<(), ServeError>
where
  F: Future<Output = ()> + Send,
{
  let address = validate_listener(&listener)?;
  debug_assert!(address.ip().is_loopback());
  let stream_shutdown = CancellationToken::new();
  let service = AgentServiceServer::new(
    AgentGrpcService::new_with_identity(host, server_instance_id, scope_fingerprint)
      .with_stream_shutdown(stream_shutdown.clone()),
  )
  .max_decoding_message_size(MAX_GRPC_MESSAGE_BYTES)
  .max_encoding_message_size(MAX_GRPC_MESSAGE_BYTES);
  let service =
    tonic::service::interceptor::InterceptedService::new(service, ServerAuthorization::new(token));
  let shutdown = cancel_streams_on_shutdown(shutdown, stream_shutdown);
  tonic::transport::Server::builder()
    .add_service(service)
    .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
    .await
    .map_err(ServeError::Transport)
}

fn validate_listener(listener: &TcpListener) -> Result<SocketAddr, ServeError> {
  let address = listener.local_addr().map_err(ServeError::Io)?;
  if address.ip().is_loopback() {
    Ok(address)
  } else {
    Err(ServeError::NonLoopback(address))
  }
}

async fn cancel_streams_on_shutdown<F>(shutdown: F, stream_shutdown: CancellationToken)
where
  F: Future<Output = ()>,
{
  shutdown.await;
  stream_shutdown.cancel();
}
