use crate::{Handshake, ProbeError, ReadinessProbe};
use agent_client::{AgentClient, AuthenticatedTarget};
use agent_local::{DiscoveryDescriptor, ScopeFingerprint};
use agent_protocol::PROTOCOL_VERSION;
use async_trait::async_trait;
use std::time::Duration;
use tonic::Code;
use uuid::Uuid;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct GrpcReadinessProbe {
  connect_timeout: Duration,
  rpc_timeout: Duration,
}

impl Default for GrpcReadinessProbe {
  fn default() -> Self {
    Self {
      connect_timeout: DEFAULT_CONNECT_TIMEOUT,
      rpc_timeout: DEFAULT_RPC_TIMEOUT,
    }
  }
}

impl GrpcReadinessProbe {
  pub fn with_timeouts(connect_timeout: Duration, rpc_timeout: Duration) -> Self {
    Self {
      connect_timeout,
      rpc_timeout,
    }
  }
}

#[async_trait]
impl ReadinessProbe for GrpcReadinessProbe {
  async fn probe(&self, descriptor: &DiscoveryDescriptor) -> Result<Handshake, ProbeError> {
    let target = AuthenticatedTarget::from_descriptor(descriptor);
    let connect = AgentClient::connect(target, "agent-launch-readiness");
    let mut client = tokio::time::timeout(self.connect_timeout, connect)
      .await
      .map_err(|_| ProbeError::Unavailable)?
      .map_err(|_| ProbeError::Unavailable)?;
    let response = tokio::time::timeout(
      self.rpc_timeout,
      client.handshake("agent-launch", env!("CARGO_PKG_VERSION")),
    )
    .await
    .map_err(|_| ProbeError::Unavailable)?
    .map_err(map_status)?;
    if response.protocol_version != PROTOCOL_VERSION {
      return Err(ProbeError::HandshakeMismatch {
        reason: "protocol version differs",
      });
    }
    let server_instance_id =
      Uuid::parse_str(&response.server_instance_id).map_err(|_| ProbeError::HandshakeMismatch {
        reason: "server instance id is invalid",
      })?;
    let scope_fingerprint = ScopeFingerprint::parse(&response.scope_fingerprint).map_err(|_| {
      ProbeError::HandshakeMismatch {
        reason: "scope fingerprint is invalid",
      }
    })?;
    Ok(Handshake::new(server_instance_id, scope_fingerprint))
  }
}

fn map_status(status: tonic::Status) -> ProbeError {
  match status.code() {
    Code::Unauthenticated | Code::PermissionDenied => ProbeError::AuthenticationRejected,
    Code::FailedPrecondition | Code::InvalidArgument => ProbeError::HandshakeMismatch {
      reason: "daemon rejected the handshake",
    },
    _ => ProbeError::Unavailable,
  }
}
