use std::time::Instant;

use agent_core::{ToolInvocation, ToolResult};

use crate::{ToolContext, ToolError, ToolRegistry};

pub struct ToolExecutor {
    registry: ToolRegistry,
    context: ToolContext,
}

impl ToolExecutor {
    pub fn new(registry: ToolRegistry, context: ToolContext) -> Self {
        Self { registry, context }
    }

    pub async fn execute(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<ToolResult, ToolExecutionError> {
        let tool = self
            .registry
            .get(&invocation.name)
            .ok_or_else(|| ToolExecutionError::NotFound(invocation.name.clone()))?;
        let started = Instant::now();
        let invocation_future = tool.invoke(&self.context, invocation);

        let mut result = tokio::select! {
            () = self.context.cancel.cancelled() => return Err(ToolExecutionError::Cancelled),
            result = tokio::time::timeout(self.context.timeout, invocation_future) => {
                match result {
                    Ok(result) => result.map_err(ToolExecutionError::Tool)?,
                    Err(_) => return Err(ToolExecutionError::TimedOut {
                        tool: invocation.name.clone(),
                    }),
                }
            }
        };
        result.duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        Ok(result)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolExecutionError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("tool timed out: {tool}")]
    TimedOut { tool: String },
    #[error("tool execution cancelled")]
    Cancelled,
    #[error(transparent)]
    Tool(#[from] ToolError),
}
