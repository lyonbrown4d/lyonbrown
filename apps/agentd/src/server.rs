use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use agent_protocol::{MAX_GRPC_MESSAGE_BYTES, v1::agent_service_server::AgentServiceServer};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;

use crate::{AgentGrpcService, RuntimeHost};

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

pub async fn serve<F>(
  listener: TcpListener,
  host: Arc<dyn RuntimeHost>,
  shutdown: F,
) -> Result<(), ServeError>
where
  F: Future<Output = ()> + Send,
{
  let address = listener.local_addr().map_err(ServeError::Io)?;
  if !address.ip().is_loopback() {
    return Err(ServeError::NonLoopback(address));
  }
  let stream_shutdown = CancellationToken::new();
  let service = AgentServiceServer::new(
    AgentGrpcService::new(host).with_stream_shutdown(stream_shutdown.clone()),
  )
  .max_decoding_message_size(MAX_GRPC_MESSAGE_BYTES)
  .max_encoding_message_size(MAX_GRPC_MESSAGE_BYTES);
  let shutdown = async move {
    shutdown.await;
    stream_shutdown.cancel();
  };
  tonic::transport::Server::builder()
    .add_service(service)
    .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
    .await
    .map_err(ServeError::Transport)
}
