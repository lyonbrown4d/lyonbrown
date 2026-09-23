use rmcp::model::{
  CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
  PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler, ServiceExt};

struct Fixture;

impl ServerHandler for Fixture {
  fn get_info(&self) -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
  }

  async fn list_tools(
    &self,
    _request: Option<PaginatedRequestParams>,
    _context: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, ErrorData> {
    let mut result = ListToolsResult::default();
    let schema: serde_json::Map<String, serde_json::Value> =
      serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {"text": {"type": "string"}}
      }))
      .unwrap();
    result.tools = vec![
      Tool::new("echo", "Echo a string", schema),
      Tool::new("crash", "Exit without a result", serde_json::Map::new()),
    ];
    Ok(result)
  }

  async fn call_tool(
    &self,
    request: CallToolRequestParams,
    _context: RequestContext<RoleServer>,
  ) -> Result<CallToolResponse, ErrorData> {
    if request.name == "crash" {
      std::process::exit(1);
    }
    let text = request
      .arguments
      .as_ref()
      .and_then(|arguments| arguments.get("text"))
      .and_then(serde_json::Value::as_str)
      .unwrap_or_default();
    Ok(CallToolResponse::Complete(CallToolResult::success(vec![
      ContentBlock::text(text),
    ])))
  }
}

#[tokio::main]
async fn main() {
  let server = Fixture.serve(rmcp::transport::io::stdio()).await.unwrap();
  server.waiting().await.unwrap();
}
