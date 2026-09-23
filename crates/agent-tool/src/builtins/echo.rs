use super::*;
use crate::Tool;
use agent_core::{RiskLevel, ToolDefinition, ToolSource};
use async_trait::async_trait;
use serde_json::json;
use std::io::Read;

pub struct EchoTool {
  definition: ToolDefinition,
}

impl EchoTool {
  pub fn new() -> Self {
    Self {
      definition: ToolDefinition {
        name: "echo".into(),
        description: "Return the supplied text unchanged.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
            "additionalProperties": false
        }),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Read,
        requires_secrets: vec![],
      },
    }
  }
}

impl Default for EchoTool {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl Tool for EchoTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    _context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    let text = invocation
      .arguments
      .get("text")
      .and_then(serde_json::Value::as_str)
      .ok_or_else(|| ToolError::InvalidArguments("text must be a string".into()))?;
    Ok(ToolResult {
      invocation_id: invocation.id,
      ok: true,
      output: text.to_owned(),
      truncated: false,
      duration_ms: 0,
      error: None,
    })
  }
}

pub struct ReadFileTool {
  definition: ToolDefinition,
}

impl ReadFileTool {
  pub fn new() -> Self {
    Self {
      definition: ToolDefinition {
        name: "read_file".into(),
        description: "Read a UTF-8 file inside the workspace root.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false
        }),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Read,
        requires_secrets: vec![],
      },
    }
  }
}

impl Default for ReadFileTool {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl Tool for ReadFileTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    let raw_path = invocation
      .arguments
      .get("path")
      .and_then(serde_json::Value::as_str)
      .ok_or_else(|| ToolError::InvalidArguments("path must be a string".into()))?;
    let directory = context.workspace_dir()?;
    let relative = validate_relative_path(raw_path)?.to_owned();
    let output = tokio::task::spawn_blocking(move || {
      let mut file = directory.open(&relative)?;
      let mut output = String::new();
      file.read_to_string(&mut output)?;
      Ok::<_, std::io::Error>(output)
    })
    .await
    .map_err(|error| ToolError::Io(error.to_string()))?
    .map_err(|error| ToolError::Io(error.to_string()))?;

    Ok(ToolResult {
      invocation_id: invocation.id,
      ok: true,
      output,
      truncated: false,
      duration_ms: 0,
      error: None,
    })
  }
}
