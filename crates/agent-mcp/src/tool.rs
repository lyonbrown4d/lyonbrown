use std::{
  collections::{BTreeMap, BTreeSet},
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use agent_runtime::{Result, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use async_trait::async_trait;
use rmcp::{
  model::{CallToolRequest, CallToolRequestParams, ClientRequest, ServerResult},
  service::PeerRequestOptions,
};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
  McpLimits, McpServerConfig, catalog::CatalogEntry, config::invalid, connection::Connection,
};

/// Exact server identity and argument value already validated against its cached
/// schema. The daemon must durably authorize this intent before returning Ok.
#[derive(Clone, Debug)]
pub struct McpApproval {
  pub server_id: String,
  pub tool_name: String,
  pub call_id: String,
  pub arguments: Value,
}

#[async_trait]
pub trait McpAuthorization: Send + Sync {
  async fn approve(&self, call: McpApproval, cancellation: CancellationToken) -> Result<()>;
}

pub(crate) struct Server {
  pub connection: Connection,
  pub catalog: BTreeMap<String, CatalogEntry>,
}

/// A daemon-owned manager with one cached, explicitly allowed catalog. Clients
/// cannot add servers, supply transport configuration or issue arbitrary RPC.
pub struct McpManager {
  servers: BTreeMap<String, Server>,
  authorization: Arc<dyn McpAuthorization>,
  limits: McpLimits,
  closed: AtomicBool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
  call: Invocation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
  server: String,
  tool: String,
  arguments: Value,
}

impl McpManager {
  pub async fn connect(
    configs: Vec<McpServerConfig>,
    authorization: Arc<dyn McpAuthorization>,
    limits: McpLimits,
  ) -> Result<Self> {
    limits.validate()?;
    if configs.is_empty() || configs.len() > 64 {
      return Err(invalid("configure between 1 and 64 MCP servers"));
    }
    let mut ids = BTreeSet::new();
    let mut total = 0;
    for config in &configs {
      config.validate()?;
      total += config.allowed_tools.len();
      if !ids.insert(&config.id) || total > limits.max_tools {
        return Err(invalid(
          "duplicate MCP server or selected catalog exceeds limit",
        ));
      }
    }
    let mut servers = BTreeMap::new();
    for config in configs {
      let connection = crate::connection::connect(&config, &limits).await?;
      let catalog = tokio::time::timeout(
        limits.connection_timeout,
        crate::catalog::catalog(&connection, &config, &limits),
      )
      .await
      .map_err(|_| invalid("MCP catalog deadline exceeded"))??;
      tracing::info!(server_id = %config.id, selected_tools = catalog.len(), outcome = "connected", "MCP server connected");
      servers.insert(
        config.id,
        Server {
          connection,
          catalog,
        },
      );
    }
    Ok(Self {
      servers,
      authorization,
      limits,
      closed: AtomicBool::new(false),
    })
  }

  /// Bind a run-specific approval port while sharing daemon-owned connections.
  pub fn tool_for(
    self: &Arc<Self>,
    authorization: Arc<dyn McpAuthorization>,
  ) -> Arc<dyn RuntimeTool> {
    Arc::new(RunTool {
      manager: self.clone(),
      authorization,
    })
  }

  /// Stop daemon-owned connections even while run tools retain this manager.
  /// Calls already dispatched have an unknown outcome and must be reconciled.
  pub fn cancel_connections(&self) {
    self.closed.store(true, Ordering::Release);
    for server in self.servers.values() {
      server.connection.cancellation_token().cancel();
    }
  }

  pub async fn shutdown(self) {
    self.cancel_connections();
    for (id, server) in self.servers {
      let _ = tokio::time::timeout(Duration::from_secs(5), server.connection.cancel()).await;
      tracing::info!(server_id = %id, outcome = "closed", "MCP server connection closed");
    }
  }
}

struct RunTool {
  manager: Arc<McpManager>,
  authorization: Arc<dyn McpAuthorization>,
}

#[async_trait]
impl RuntimeTool for RunTool {
  fn definition(&self) -> ToolDefinition {
    self.manager.definition()
  }

  async fn execute(&self, call: ToolCall, cancellation: CancellationToken) -> Result<ToolOutput> {
    self
      .manager
      .execute_authorized(call, cancellation, self.authorization.as_ref())
      .await
  }
}

#[async_trait]
impl RuntimeTool for McpManager {
  fn definition(&self) -> ToolDefinition {
    crate::catalog::definition(&self.servers)
  }

  async fn execute(&self, call: ToolCall, cancellation: CancellationToken) -> Result<ToolOutput> {
    self
      .execute_authorized(call, cancellation, self.authorization.as_ref())
      .await
  }
}

impl McpManager {
  async fn execute_authorized(
    &self,
    call: ToolCall,
    cancellation: CancellationToken,
    authorization: &dyn McpAuthorization,
  ) -> Result<ToolOutput> {
    if self.closed.load(Ordering::Acquire) {
      return Err(RuntimeError::Tool("MCP connections are closed".into()));
    }
    if cancellation.is_cancelled() {
      return Err(RuntimeError::Tool(
        "MCP call cancelled before authorization".into(),
      ));
    }
    let Arguments { call: args } = serde_json::from_value(call.arguments)
      .map_err(|_| RuntimeError::Tool("invalid MCP call arguments".into()))?;
    let server = self
      .servers
      .get(&args.server)
      .ok_or_else(|| RuntimeError::Tool("MCP server is not allowed".into()))?;
    let entry = server
      .catalog
      .get(&args.tool)
      .ok_or_else(|| RuntimeError::Tool("MCP tool is not allowed".into()))?;
    if !args.arguments.is_object()
      || !entry.validator.is_valid(&args.arguments)
      || serde_json::to_vec(&args.arguments)
        .map_err(|_| RuntimeError::Tool("invalid MCP arguments".into()))?
        .len()
        > self.limits.max_text_bytes
    {
      return Err(RuntimeError::Tool(
        "MCP arguments violate the selected tool schema or size limit".into(),
      ));
    }
    let approval = McpApproval {
      server_id: args.server.clone(),
      tool_name: args.tool.clone(),
      call_id: call.id.clone(),
      arguments: args.arguments.clone(),
    };
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => return Err(RuntimeError::Tool("MCP authorization cancelled".into())),
      result = tokio::time::timeout(self.limits.call_timeout, authorization.approve(approval, cancellation.clone())) => {
        result.map_err(|_| RuntimeError::Tool("MCP authorization deadline exceeded".into()))??;
      }
    }
    if self.closed.load(Ordering::Acquire) {
      return Err(RuntimeError::Tool(
        "MCP connections closed before dispatch".into(),
      ));
    }
    if cancellation.is_cancelled() {
      return Err(RuntimeError::Tool(
        "MCP call cancelled before dispatch".into(),
      ));
    }
    let params = CallToolRequestParams::new(args.tool.clone()).with_arguments(
      args
        .arguments
        .as_object()
        .cloned()
        .ok_or_else(|| RuntimeError::Tool("MCP arguments must be an object".into()))?,
    );
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
    let future = server
      .connection
      .send_cancellable_request(request, PeerRequestOptions::no_options());
    let mut handle = tokio::time::timeout(self.limits.call_timeout, future)
      .await
      .map_err(|_| uncertain())?
      .map_err(|_| uncertain())?;
    tracing::info!(server_id = %args.server, tool_name = %args.tool, call_id = %call.id, outcome = "dispatched", "MCP tool call dispatched");
    let response = tokio::select! {
      biased;
      result = &mut handle.rx => result.map_err(|_| uncertain())?.map_err(|_| uncertain())?,
      _ = cancellation.cancelled() => {
        let _ = tokio::time::timeout(Duration::from_secs(1), handle.cancel(Some("daemon cancellation".into()))).await;
        return Err(uncertain());
      }
      _ = tokio::time::sleep(self.limits.call_timeout) => {
        let _ = tokio::time::timeout(Duration::from_secs(1), handle.cancel(Some("daemon deadline exceeded".into()))).await;
        return Err(uncertain());
      }
    };
    match response {
      ServerResult::CallToolResult(result) => crate::output::output(result, &self.limits),
      // Tasks and multi-round input would create additional execution paths.
      _ => Err(RuntimeError::Uncertain(
        "MCP server returned an unsupported non-final result; reconcile before continuing".into(),
      )),
    }
  }
}

fn uncertain() -> RuntimeError {
  RuntimeError::Uncertain("MCP call may have executed; reconcile server state before continuing; the request was not replayed".into())
}
