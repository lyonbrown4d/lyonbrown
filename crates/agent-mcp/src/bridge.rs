use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_core::{RiskLevel, ToolDefinition, ToolInvocation, ToolResult, ToolSource};
use agent_tool::{Tool, ToolContext, ToolError, ToolRegistry};
use async_trait::async_trait;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ContentBlock};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;

use crate::McpServerConfig;

type Client = RunningService<RoleClient, ()>;
const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum McpError {
  #[error("invalid MCP server name: {0}")]
  InvalidName(String),
  #[error("MCP server {0} failed: {1}")]
  Server(String, String),
}

pub struct McpBridge {
  client: Arc<Client>,
  tools: Vec<ToolDefinition>,
}

impl McpBridge {
  pub async fn connect(name: &str, config: &McpServerConfig) -> Result<Self, McpError> {
    if name.is_empty()
      || !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
      return Err(McpError::InvalidName(name.into()));
    }
    let mut command = Command::new(&config.command);
    command.args(&config.args);
    command.kill_on_drop(true);
    let transport = TokioChildProcess::new(command)
      .map_err(|error| McpError::Server(name.into(), error.to_string()))?;
    let client = tokio::time::timeout(TIMEOUT, ().serve(transport))
      .await
      .map_err(|error| McpError::Server(name.into(), error.to_string()))?
      .map_err(|error| McpError::Server(name.into(), error.to_string()))?;
    let tools = tokio::time::timeout(TIMEOUT, client.peer().list_all_tools())
      .await
      .map_err(|error| McpError::Server(name.into(), error.to_string()))?
      .map_err(|error| McpError::Server(name.into(), error.to_string()))?
      .into_iter()
      .map(|tool| ToolDefinition {
        name: format!("mcp.{name}.{}", tool.name),
        description: tool.description.as_deref().unwrap_or_default().into(),
        input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
        source: ToolSource::Mcp {
          server: name.into(),
        },
        risk: RiskLevel::Execute,
        requires_secrets: Vec::new(),
      })
      .collect();
    Ok(Self {
      client: Arc::new(client),
      tools,
    })
  }

  pub fn register(&self, registry: &ToolRegistry) {
    for definition in &self.tools {
      let original_name = definition
        .name
        .splitn(3, '.')
        .nth(2)
        .expect("MCP tool names have a prefix and server name")
        .to_owned();
      registry.register(Arc::new(McpTool {
        client: self.client.clone(),
        original_name,
        definition: definition.clone(),
      }));
    }
  }

  pub fn definitions(&self) -> &[ToolDefinition] {
    &self.tools
  }
}

struct McpTool {
  client: Arc<Client>,
  original_name: String,
  definition: ToolDefinition,
}

#[async_trait]
impl Tool for McpTool {
  fn definition(&self) -> &ToolDefinition {
    &self.definition
  }

  async fn invoke(
    &self,
    context: &ToolContext,
    invocation: &ToolInvocation,
  ) -> Result<ToolResult, ToolError> {
    let arguments = invocation.arguments.as_object().cloned().ok_or_else(|| {
      ToolError::InvalidArguments("MCP tool arguments must be a JSON object".into())
    })?;
    let started = Instant::now();
    let call = self
      .client
      .peer()
      .call_tool(CallToolRequestParams::new(self.original_name.clone()).with_arguments(arguments));
    let response = tokio::select! {
      () = context.cancel.cancelled() => return Err(ToolError::Execution("MCP call cancelled".into())),
      result = tokio::time::timeout(TIMEOUT.min(context.timeout), call) => result
        .map_err(|_| ToolError::Execution("MCP call timed out".into()))?
        .map_err(|error| ToolError::Execution(error.to_string()))?,
    };
    let mut output = response
      .content
      .iter()
      .map(|content| match content {
        ContentBlock::Text(text) => text.text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
      })
      .collect::<Vec<_>>()
      .join("\n");
    if output.is_empty()
      && let Some(structured) = response.structured_content
    {
      output = structured.to_string();
    }
    Ok(ToolResult {
      invocation_id: invocation.id,
      ok: response.is_error != Some(true),
      output,
      truncated: false,
      duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
      error: (response.is_error == Some(true)).then(|| "MCP tool returned an error".into()),
    })
  }
}
