use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_core::{ToolDefinition, ToolInvocation, ToolResult};
use agent_secret::SecretBroker;
use async_trait::async_trait;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ToolContext {
  pub workspace_root: PathBuf,
  pub broker: Arc<dyn SecretBroker>,
  pub cancel: CancellationToken,
  pub timeout: Duration,
  workspace_dir: Result<Arc<Dir>, Arc<str>>,
}

impl ToolContext {
  pub fn new(
    workspace_root: PathBuf,
    broker: Arc<dyn SecretBroker>,
    cancel: CancellationToken,
    timeout: Duration,
  ) -> Self {
    let workspace_dir = Dir::open_ambient_dir(&workspace_root, ambient_authority())
      .map(Arc::new)
      .map_err(|error| Arc::<str>::from(error.to_string()));
    Self {
      workspace_root,
      broker,
      cancel,
      timeout,
      workspace_dir,
    }
  }

  pub(crate) fn workspace_dir(&self) -> Result<Arc<Dir>, ToolError> {
    self
      .workspace_dir
      .clone()
      .map_err(|error| ToolError::Io(error.to_string()))
  }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
  #[error("invalid tool arguments: {0}")]
  InvalidArguments(String),
  #[error("tool I/O failed: {0}")]
  Io(String),
  #[error("tool failed: {0}")]
  Execution(String),
}

#[async_trait]
pub trait Tool: Send + Sync {
  fn definition(&self) -> &ToolDefinition;
  async fn invoke(
    &self,
    context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError>;
}
