use std::path::{Component, Path};

use agent_core::{RiskLevel, ToolDefinition, ToolInvocation, ToolResult, ToolSource};
use async_trait::async_trait;
use serde_json::json;

use crate::{Tool, ToolContext, ToolError};

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
        let relative = Path::new(raw_path);
        if raw_path.is_empty()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(ToolError::InvalidArguments(
                "path must stay inside the workspace root".into(),
            ));
        }

        // M1/M3 的路径监狱按计划使用 canonicalize + component-aware starts_with。
        // hostile mutable workspace 仍可能在检查与打开之间替换 symlink/junction；
        // handle-relative/no-follow 打开属于 M3 的平台级文件系统硬化。
        let root = context.workspace_root.clone();
        let relative = relative.to_owned();
        let output = tokio::task::spawn_blocking(move || {
            let canonical_root = root.canonicalize()?;
            let canonical_path = canonical_root.join(relative).canonicalize()?;
            if !canonical_path.starts_with(&canonical_root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "path escapes workspace root",
                ));
            }
            std::fs::read_to_string(canonical_path)
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
