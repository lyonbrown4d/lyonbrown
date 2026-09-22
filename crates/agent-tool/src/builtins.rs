use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Component, Path};
use std::process::Stdio;

use agent_core::{RiskLevel, SecretRef, ToolDefinition, ToolInvocation, ToolResult, ToolSource};
use async_trait::async_trait;
use cap_std::fs::OpenOptions;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

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
