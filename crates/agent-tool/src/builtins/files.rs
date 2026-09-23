use super::*;
use crate::Tool;
use agent_core::{RiskLevel, ToolDefinition, ToolSource};
use async_trait::async_trait;
use cap_std::fs::OpenOptions;
use serde_json::json;
use std::io::Write;

pub struct WriteFileTool {
  definition: ToolDefinition,
}

impl WriteFileTool {
  pub fn new() -> Self {
    Self {
      definition: ToolDefinition {
        name: "write_file".into(),
        description: "Write UTF-8 content to a file inside the workspace root.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Write,
        requires_secrets: vec![],
      },
    }
  }
}

impl Default for WriteFileTool {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl Tool for WriteFileTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    let raw_path = string_argument(invocation, "path")?;
    let content = string_argument(invocation, "content")?.to_owned();
    let relative = validate_relative_path(raw_path)?.to_owned();
    let directory = context.workspace_dir()?;
    let byte_count = content.len();
    tokio::task::spawn_blocking(move || {
      let mut options = OpenOptions::new();
      options.write(true).create(true).truncate(true);
      let mut file = directory.open_with(&relative, &options)?;
      file.write_all(content.as_bytes())
    })
    .await
    .map_err(|error| ToolError::Io(error.to_string()))?
    .map_err(|error| ToolError::Io(error.to_string()))?;
    Ok(success_result(
      invocation,
      format!("wrote {byte_count} bytes"),
      false,
    ))
  }
}

pub struct ListFilesTool {
  definition: ToolDefinition,
}

impl ListFilesTool {
  pub fn new() -> Self {
    Self {
      definition: ToolDefinition {
        name: "list_files".into(),
        description: "List entries in a directory inside the workspace root.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {"path": {"type": "string", "default": "."}},
            "additionalProperties": false
        }),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Read,
        requires_secrets: vec![],
      },
    }
  }
}

impl Default for ListFilesTool {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl Tool for ListFilesTool {
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
      .map(|value| {
        value
          .as_str()
          .ok_or_else(|| ToolError::InvalidArguments("path must be a string".into()))
      })
      .transpose()?
      .unwrap_or(".");
    let relative = validate_relative_path(raw_path)?.to_owned();
    let workspace = context.workspace_dir()?;
    let output = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
      let mut entries = Vec::new();
      for entry in workspace.read_dir(&relative)? {
        let entry = entry?;
        let mut name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() {
          name.push('/');
        }
        entries.push(name);
      }
      entries.sort();
      Ok(entries.join("\n"))
    })
    .await
    .map_err(|error| ToolError::Io(error.to_string()))?
    .map_err(|error| ToolError::Io(error.to_string()))?;
    Ok(success_result(invocation, output, false))
  }
}
