use std::sync::Arc;

use agent_client::{
  AgentClientError, AuthenticatedTarget, AuthenticatedTargetResolver, AvailabilityKind,
  ProtocolErrorKind,
};
use async_trait::async_trait;

use crate::{LaunchCoordinator, LaunchError, LaunchRequest};

/// Re-runs secure discovery and optional daemon startup for every client epoch.
pub struct CoordinatorTargetResolver {
  coordinator: Arc<LaunchCoordinator>,
  request: LaunchRequest,
}

impl CoordinatorTargetResolver {
  pub fn new(coordinator: Arc<LaunchCoordinator>, request: LaunchRequest) -> Self {
    Self {
      coordinator,
      request,
    }
  }
}

#[async_trait]
impl AuthenticatedTargetResolver for CoordinatorTargetResolver {
  async fn resolve(&self) -> Result<AuthenticatedTarget, AgentClientError> {
    self
      .coordinator
      .connect_or_start(&self.request)
      .await
      .map(|target| target.authenticated_target())
      .map_err(map_launch_error)
  }
}

fn map_launch_error(error: LaunchError) -> AgentClientError {
  let message = error.to_string();
  match error {
    LaunchError::AuthenticationRejected => {
      AgentClientError::protocol(ProtocolErrorKind::AuthenticationFailed, message)
    }
    LaunchError::ScopeMismatch { .. } | LaunchError::HandshakeMismatch { .. } => {
      AgentClientError::protocol(ProtocolErrorKind::IncompatibleVersion, message)
    }
    _ => AgentClientError::unavailable(AvailabilityKind::Disconnected, message),
  }
}
