use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::{Value, json};
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
pub struct Fixture {
  pub calls: Arc<AtomicUsize>,
  pub arguments: Arc<tokio::sync::Mutex<Vec<Value>>>,
  pub started: Arc<tokio::sync::Notify>,
  pub cancelled: Arc<tokio::sync::Notify>,
}

impl ServerHandler for Fixture {
  fn get_info(&self) -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
  }

  async fn list_tools(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> std::result::Result<ListToolsResult, ErrorData> {
    let tool: Tool = serde_json::from_value(json!({
      "name": "echo", "description": "Untrusted echo fixture",
      "inputSchema": {"type": "object", "required": ["message"], "additionalProperties": false,
        "$defs": {"Message": {"type": "string"}}, "properties": {"message": {"$ref": "#/$defs/Message"}}},
      "annotations": {"readOnlyHint": true}
    }))
    .expect("fixture schema");
    Ok(ListToolsResult {
      tools: vec![tool],
      ..Default::default()
    })
  }

  async fn call_tool(
    &self,
    params: CallToolRequestParams,
    context: RequestContext<RoleServer>,
  ) -> std::result::Result<CallToolResponse, ErrorData> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    self.started.notify_one();
    let arguments = Value::Object(params.arguments.unwrap_or_default());
    self.arguments.lock().await.push(arguments.clone());
    let message = arguments["message"].as_str().unwrap_or_default();
    if message == "pid" {
      return Ok(
        CallToolResult::success(vec![ContentBlock::text(std::process::id().to_string())]).into(),
      );
    }
    if message == "image" {
      return Ok(
        CallToolResult::success(vec![ContentBlock::image("AAAAAAAAAAAA", "image/png")]).into(),
      );
    }
    if message == "callbacks" {
      let sampling: ServerRequest = serde_json::from_value(json!({
        "method": "sampling/createMessage", "params": {"messages": [{"role": "user", "content": {"type": "text", "text": "unapproved"}}], "maxTokens": 8}
      })).expect("sampling fixture request");
      let sampling_denied = context.peer.send_request(sampling).await.is_err();
      let elicitation: ServerRequest = serde_json::from_value(json!({
        "method": "elicitation/create", "params": {"mode": "form", "message": "unapproved", "requestedSchema": {"type": "object", "properties": {}}}
      })).expect("elicitation fixture request");
      let elicitation_denied = match context.peer.send_request(elicitation).await {
        Ok(ClientResult::ElicitResult(result)) => result.action == ElicitationAction::Decline,
        Err(_) => true,
        _ => false,
      };
      return Ok(
        CallToolResult::success(vec![ContentBlock::text(format!(
          "sampling_denied={sampling_denied};elicitation_denied={elicitation_denied}"
        ))])
        .into(),
      );
    }
    if message == "wait" {
      context.ct.cancelled().await;
      self.cancelled.notify_one();
      return Ok(CallToolResult::success(vec![ContentBlock::text("cancelled")]).into());
    }
    if message == "error" {
      return Ok(CallToolResult::error(vec![ContentBlock::text("fixture rejected")]).into());
    }
    if message == "oversize" {
      return Ok(
        CallToolResult::success(vec![ContentBlock::text("x".repeat(2 * 1024 * 1024))]).into(),
      );
    }
    if message == "wire_oversize" {
      return Ok(
        CallToolResult::success(vec![ContentBlock::text("x".repeat(25 * 1024 * 1024))]).into(),
      );
    }
    Ok(CallToolResult::success(vec![ContentBlock::text(message)]).into())
  }
}
