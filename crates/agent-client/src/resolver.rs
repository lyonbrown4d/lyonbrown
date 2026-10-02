use std::sync::Arc;

use agent_app::AgentClientError;
use async_trait::async_trait;

use crate::AuthenticatedTarget;

/// Resolves the active authenticated daemon target for each connection epoch.
#[async_trait]
pub trait AuthenticatedTargetResolver: Send + Sync {
  async fn resolve(&self) -> Result<AuthenticatedTarget, AgentClientError>;
}

struct FixedTargetResolver {
  target: AuthenticatedTarget,
}

#[async_trait]
impl AuthenticatedTargetResolver for FixedTargetResolver {
  async fn resolve(&self) -> Result<AuthenticatedTarget, AgentClientError> {
    Ok(self.target.clone())
  }
}

pub(crate) fn fixed_target_resolver(
  target: AuthenticatedTarget,
) -> Arc<dyn AuthenticatedTargetResolver> {
  Arc::new(FixedTargetResolver { target })
}
