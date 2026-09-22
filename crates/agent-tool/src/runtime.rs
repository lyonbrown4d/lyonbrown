use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_core::{ToolDefinition, ToolInvocation, ToolResult};
use agent_secret::SecretBroker;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ToolContext {
    pub workspace_root: PathBuf,
    pub broker: Arc<dyn SecretBroker>,
    pub cancel: CancellationToken,
    pub timeout: Duration,
}

impl ToolContext {
    pub fn new(
        workspace_root: PathBuf,
        broker: Arc<dyn SecretBroker>,
        cancel: CancellationToken,
        timeout: Duration,
    ) -> Self {
        Self {
            workspace_root,
            broker,
            cancel,
            timeout,
        }
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
