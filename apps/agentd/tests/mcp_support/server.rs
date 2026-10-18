use agent_mcp::{McpServerConfig, McpTransport, SecretHeaders};
use rmcp::{
  ErrorData, RoleServer, ServerHandler,
  model::*,
  service::RequestContext,
  transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
  },
};
use serde_json::{Value, json};
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct Fixture {
  pub calls: Arc<AtomicUsize>,
  pub received: Arc<tokio::sync::Mutex<Vec<Value>>>,
  pub started: Arc<tokio::sync::Notify>,
}
impl ServerHandler for Fixture {
  fn get_info(&self) -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
  }
  async fn list_tools(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, ErrorData> {
    let tool = serde_json::from_value(json!({"name":"echo", "inputSchema":{"type":"object","required":["message"],"additionalProperties":false,"properties":{"message":{"type":"string"}}},"annotations":{"readOnlyHint":true}})).expect("fixture schema");
    Ok(ListToolsResult {
      tools: vec![tool],
      ..Default::default()
    })
  }
  async fn call_tool(
    &self,
    params: CallToolRequestParams,
    context: RequestContext<RoleServer>,
  ) -> Result<CallToolResponse, ErrorData> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    let value = Value::Object(params.arguments.unwrap_or_default());
    self.received.lock().await.push(value.clone());
    self.started.notify_one();
    if value["message"] == "wait" {
      context.ct.cancelled().await;
    }
    Ok(
      CallToolResult::success(vec![ContentBlock::text(
        value["message"].as_str().unwrap_or_default(),
      )])
      .into(),
    )
  }
}
impl Fixture {
  pub async fn start(&self) -> (McpServerConfig, CancellationToken) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
      .await
      .expect("MCP loopback");
    let address = listener.local_addr().expect("MCP address");
    let stop = CancellationToken::new();
    let mut options = StreamableHttpServerConfig::default();
    options.cancellation_token = stop.clone();
    let fixture = self.clone();
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
        .expect("MCP fixture");
    });
    (
      McpServerConfig {
        id: "fixture".into(),
        transport: McpTransport::StreamableHttp {
          url: format!("http://{address}/mcp"),
          headers: SecretHeaders::default(),
        },
        allowed_tools: ["echo".into()].into_iter().collect(),
      },
      stop,
    )
  }
}
