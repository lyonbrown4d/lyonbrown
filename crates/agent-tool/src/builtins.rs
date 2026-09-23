use std::collections::BTreeMap;
use std::path::{Component, Path};

use agent_core::{SecretRef, ToolInvocation, ToolResult};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::{ToolContext, ToolError};

mod echo;
mod files;
mod shell;
mod workspace_search;

pub use echo::{EchoTool, ReadFileTool};
pub use files::{ListFilesTool, WriteFileTool};
pub use shell::ShellTool;
pub use workspace_search::SearchWorkspaceTool;

fn string_argument<'a>(invocation: &'a ToolInvocation, name: &str) -> Result<&'a str, ToolError> {
  invocation
    .arguments
    .get(name)
    .and_then(serde_json::Value::as_str)
    .ok_or_else(|| ToolError::InvalidArguments(format!("{name} must be a string")))
}

fn validate_relative_path(raw: &str) -> Result<&Path, ToolError> {
  let relative = Path::new(raw);
  if raw.is_empty()
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
  Ok(relative)
}

fn success_result(invocation: &ToolInvocation, output: String, truncated: bool) -> ToolResult {
  ToolResult {
    invocation_id: invocation.id,
    ok: true,
    output,
    truncated,
    duration_ms: 0,
    error: None,
  }
}

fn platform_shell(command_text: &str) -> Command {
  #[cfg(windows)]
  {
    let mut command = Command::new("cmd.exe");
    command.args(["/D", "/S", "/C", command_text]);
    command
  }
  #[cfg(not(windows))]
  {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", command_text]);
    command
  }
}

async fn read_capped(
  mut reader: impl AsyncRead + Unpin,
  limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
  let mut kept = Vec::with_capacity(limit);
  let mut truncated = false;
  let mut buffer = [0_u8; 8 * 1024];
  loop {
    let read = reader.read(&mut buffer).await?;
    if read == 0 {
      break;
    }
    let remaining = limit.saturating_sub(kept.len());
    let copy = remaining.min(read);
    kept.extend_from_slice(&buffer[..copy]);
    truncated |= copy < read;
  }
  Ok((kept, truncated))
}

async fn resolve_environment(
  context: &ToolContext,
  invocation: &ToolInvocation,
) -> Result<BTreeMap<String, String>, ToolError> {
  let Some(environment) = invocation.arguments.get("env") else {
    return Ok(BTreeMap::new());
  };
  let environment = environment
    .as_object()
    .ok_or_else(|| ToolError::InvalidArguments("env must be an object".into()))?;
  let mut resolved = BTreeMap::new();
  for (name, value) in environment {
    if name.is_empty() || name.contains(['=', '\0']) {
      return Err(ToolError::InvalidArguments(
        "environment variable name is invalid".into(),
      ));
    }
    let encoded = value.as_str().ok_or_else(|| {
      ToolError::InvalidArguments("environment values must be secret references".into())
    })?;
    let reference = encoded.strip_prefix("secret:").ok_or_else(|| {
      ToolError::InvalidArguments("environment values must start with secret:".into())
    })?;
    let reference = SecretRef(reference.to_owned());
    let (kind, secret_name) = reference.parts();
    if !matches!(kind, "env" | "file") || secret_name.is_empty() {
      return Err(ToolError::InvalidArguments(
        "secret reference must be env:NAME or file:NAME".into(),
      ));
    }
    let value = context
      .broker
      .resolve(&reference)
      .await
      .map_err(|error| ToolError::Execution(error.to_string()))?;
    resolved.insert(name.clone(), value);
  }
  Ok(resolved)
}
