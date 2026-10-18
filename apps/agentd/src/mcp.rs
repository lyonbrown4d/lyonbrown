//! MCP connections are composed once per daemon, with per-run approval ports.
use crate::{Daemon, config::Config};
use agent_mcp::{McpApproval, McpAuthorization, McpLimits, McpManager};
use agent_runtime::{RuntimeError, RuntimeTool};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(crate) struct Connections(tokio::sync::OnceCell<Arc<McpManager>>);

impl Connections {
  pub async fn initialize(&self, config: &Config) -> anyhow::Result<()> {
    agent_security::initialize_tls();
    if config.mcp.is_empty() {
      return Ok(());
    }
    self
      .0
      .get_or_try_init(|| async {
        McpManager::connect(
          config.mcp.clone(),
          Arc::new(DenyUnscoped),
          McpLimits::default(),
        )
        .await
        .map(Arc::new)
      })
      .await?;
    Ok(())
  }
  pub fn tool(
    &self,
    daemon: Daemon,
    run: &agent_core::Run,
    epoch: u64,
  ) -> Option<Arc<dyn RuntimeTool>> {
    self.0.get().map(|manager| {
      manager.tool_for(Arc::new(RunApproval {
        daemon,
        run: run.id.clone(),
        epoch,
      }))
    })
  }
  pub fn cancel(&self) {
    if let Some(manager) = self.0.get() {
      manager.cancel_connections();
    }
  }
}

struct DenyUnscoped;
#[async_trait::async_trait]
impl McpAuthorization for DenyUnscoped {
  async fn approve(&self, _: McpApproval, _: CancellationToken) -> agent_runtime::Result<()> {
    Err(RuntimeError::Tool(
      "MCP calls require a run-specific approval context".into(),
    ))
  }
}

struct RunApproval {
  daemon: Daemon,
  run: String,
  epoch: u64,
}

#[async_trait::async_trait]
impl McpAuthorization for RunApproval {
  async fn approve(
    &self,
    call: McpApproval,
    cancellation: CancellationToken,
  ) -> agent_runtime::Result<()> {
    let exact = serde_json::json!({"server":call.server_id,"tool":call.tool_name,"call_id":call.call_id,"arguments":call.arguments});
    crate::authorization::approve(
      &self.daemon,
      &self.run,
      self.epoch,
      "mcp_call",
      &exact,
      cancellation,
    )
    .await
    .map_err(|error| RuntimeError::Tool(error.to_string()))
  }
}
