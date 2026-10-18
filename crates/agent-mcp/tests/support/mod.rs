mod fixture;
pub use fixture::Fixture;
use std::{collections::BTreeMap, sync::Arc};

use agent_mcp::{McpApproval, McpAuthorization, McpServerConfig, McpTransport, SecretHeaders};
use agent_runtime::{Result, RuntimeError, ToolCall};
use async_trait::async_trait;
use rmcp::transport::streamable_http_server::{
  StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub struct Approval {
  pub allow: bool,
  pub seen: tokio::sync::Mutex<Vec<McpApproval>>,
}

#[async_trait]
impl McpAuthorization for Approval {
  async fn approve(&self, call: McpApproval, _: CancellationToken) -> Result<()> {
    self.seen.lock().await.push(call);
    if self.allow {
      Ok(())
    } else {
      Err(RuntimeError::Tool("approval denied".into()))
    }
  }
}

pub async fn server(fixture: Fixture, json_response: bool) -> (McpServerConfig, CancellationToken) {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("loopback bind");
  let address = listener.local_addr().expect("bound address");
  let stop = CancellationToken::new();
  let mut options = StreamableHttpServerConfig::default();
  options.json_response = json_response;
  options.cancellation_token = stop.clone();
  let service = StreamableHttpService::new(
    move || Ok(fixture.clone()),
    Arc::new(LocalSessionManager::default()),
    options,
  );
  let router = axum::Router::new().nest_service("/mcp", service);
  let shutdown = stop.clone();
  tokio::spawn(async move {
    axum::serve(listener, router)
      .with_graceful_shutdown(shutdown.cancelled_owned())
      .await
      .expect("fixture HTTP server");
  });
  (
    McpServerConfig {
      id: "fixture".into(),
      transport: McpTransport::StreamableHttp {
        url: format!("http://{address}/mcp"),
        headers: SecretHeaders(BTreeMap::new()),
      },
      allowed_tools: ["echo".into()].into_iter().collect(),
    },
    stop,
  )
}

pub fn call(message: Value) -> ToolCall {
  ToolCall {
    id: "intent-123".into(),
    arguments: json!({"call":{"server": "fixture", "tool": "echo", "arguments": {"message": message}}}),
  }
}
pub fn approval(allow: bool) -> Arc<Approval> {
  Arc::new(Approval {
    allow,
    seen: Default::default(),
  })
}
