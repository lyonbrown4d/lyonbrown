use super::*;
use crate::Tool;
use agent_core::{RiskLevel, ToolDefinition, ToolSource};
use async_trait::async_trait;
use serde_json::json;
use std::process::Stdio;

pub struct ShellTool {
  definition: ToolDefinition,
}

impl ShellTool {
  pub fn new() -> Self {
    Self {
            definition: ToolDefinition {
                name: "shell".into(),
                description: "Run one shell command in the workspace root. To use a secret, set env to an object mapping variable names to references like \"secret:env:NAME\" or \"secret:file:NAME\"; the value is resolved at execution time, injected only into the command's environment, and redacted from its output.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"},
                        "env": {
                            "type": "object",
                            "additionalProperties": {"type": "string", "pattern": "^secret:"}
                        }
                    },
                    "required": ["command"],
                    "additionalProperties": false
                }),
                source: ToolSource::BuiltIn,
                risk: RiskLevel::Execute,
                requires_secrets: vec![],
            },
        }
  }
}

impl Default for ShellTool {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl Tool for ShellTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    let command_text = string_argument(invocation, "command")?;
    if command_text.trim().is_empty() {
      return Err(ToolError::InvalidArguments(
        "command must not be empty".into(),
      ));
    }
    let environment = resolve_environment(context, invocation).await?;
    let canonical_root = context
      .workspace_root
      .canonicalize()
      .map_err(|error| ToolError::Io(error.to_string()))?;
    let mut command = platform_shell(command_text);
    command
      .current_dir(canonical_root)
      .envs(environment)
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .kill_on_drop(true);
    let mut child = command
      .spawn()
      .map_err(|error| ToolError::Execution(error.to_string()))?;
    let stdout = child
      .stdout
      .take()
      .ok_or_else(|| ToolError::Execution("failed to capture stdout".into()))?;
    let stderr = child
      .stderr
      .take()
      .ok_or_else(|| ToolError::Execution("failed to capture stderr".into()))?;
    let (stdout, stderr, status) = tokio::try_join!(
      read_capped(stdout, 24 * 1024),
      read_capped(stderr, 8 * 1024),
      child.wait()
    )
    .map_err(|error| ToolError::Execution(error.to_string()))?;
    let mut bytes = stdout.0;
    if !bytes.is_empty() && !stderr.0.is_empty() && !bytes.ends_with(b"\n") {
      bytes.push(b'\n');
    }
    bytes.extend_from_slice(&stderr.0);
    let output = String::from_utf8_lossy(&bytes).into_owned();
    let truncated = stdout.1 || stderr.1;
    let error = (!status.success()).then(|| format!("command exited with {status}"));
    Ok(ToolResult {
      invocation_id: invocation.id,
      ok: status.success(),
      output,
      truncated,
      duration_ms: 0,
      error,
    })
  }
}
