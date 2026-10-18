use std::sync::Arc;

use async_trait::async_trait;
use rpi_agent::AgentError;
use rpi_agent::agent_tool::AgentTool;
use rpi_agent::types::{AgentToolResult, ToolResultPartial};
use rpi_ai::{Schema, Tool};
use tokio_util::sync::CancellationToken;

use crate::{RuntimeError, RuntimeTool, ToolCall};

pub(crate) const UNCERTAIN_DETAIL: &str = "lyonbrown_uncertain";

pub(crate) fn uncertainty_hook() -> rpi_agent::hooks::AfterToolCall {
  Arc::new(|context, signal| {
    let uncertain = context.result.details[UNCERTAIN_DETAIL].as_bool() == Some(true);
    Box::pin(async move {
      if uncertain {
        signal.cancel();
        Some(rpi_agent::types::AfterToolCallResult {
          is_error: Some(true),
          terminate: Some(true),
          ..Default::default()
        })
      } else {
        None
      }
    })
  })
}

pub(crate) struct ToolAdapter {
  inner: Arc<dyn RuntimeTool>,
  schema: Tool,
}

impl ToolAdapter {
  pub(crate) fn new(inner: Arc<dyn RuntimeTool>) -> Self {
    let definition = inner.definition();
    let schema = Tool {
      name: definition.name,
      description: definition.description,
      parameters: Schema(definition.parameters),
      constrained_sampling: None,
    };
    Self { inner, schema }
  }
}

#[async_trait]
impl AgentTool for ToolAdapter {
  fn schema(&self) -> &Tool {
    &self.schema
  }
  fn label(&self) -> &str {
    &self.schema.name
  }

  async fn execute(
    &self,
    tool_call_id: &str,
    params: serde_json::Value,
    signal: CancellationToken,
    _on_update: Arc<dyn Fn(ToolResultPartial) + Send + Sync>,
  ) -> std::result::Result<AgentToolResult, AgentError> {
    if signal.is_cancelled() {
      return Err(AgentError::Abort);
    }
    let output = self
      .inner
      .execute(
        ToolCall {
          id: tool_call_id.into(),
          arguments: params,
        },
        signal.clone(),
      )
      .await;
    let output = match output {
      Ok(output) => output,
      Err(RuntimeError::Uncertain(message)) => {
        let mut result = AgentToolResult::error_text(message);
        result.details = serde_json::json!({ (UNCERTAIN_DETAIL): true });
        result.terminate = true;
        return Ok(result);
      }
      Err(_) if signal.is_cancelled() => return Err(AgentError::Abort),
      Err(error) => return Err(AgentError::tool(error.to_string())),
    };
    if output.is_error {
      Err(AgentError::tool(output.text))
    } else {
      Ok(AgentToolResult::text(output.text))
    }
  }
}
