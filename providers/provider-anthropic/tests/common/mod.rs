use agent_core::{Locality, ModelCapabilities, ModelTier, RiskLevel, ToolDefinition, ToolSource};
use agent_model::{ChatMessage, ModelRequest, Role};
use serde_json::json;

pub fn capabilities(model: &str) -> ModelCapabilities {
  ModelCapabilities {
    provider: "anthropic".into(),
    model: model.into(),
    tier: ModelTier::Frontier,
    locality: Locality::Remote,
    context_window: 200_000,
    max_output_tokens: 64_000,
    supports_tools: true,
    supports_streaming: true,
    supports_json: true,
    supports_thinking: true,
    cost_per_mtok_in: 3.0,
    cost_per_mtok_out: 15.0,
  }
}

pub fn request(model: &str) -> ModelRequest {
  ModelRequest {
    model: model.into(),
    system: Some("Be concise".into()),
    messages: vec![ChatMessage {
      role: Role::User,
      content: "weather".into(),
      tool_call_id: None,
      tool_name: None,
      tool_calls: vec![],
    }],
    tools: vec![ToolDefinition {
      name: "forecast".into(),
      description: "Get a forecast".into(),
      input_schema: json!({
          "type": "object",
          "properties": {"city": {"type": "string"}},
          "required": ["city"]
      }),
      source: ToolSource::BuiltIn,
      risk: RiskLevel::Network,
      requires_secrets: vec![],
    }],
    max_tokens: 2_048,
    temperature: Some(0.2),
    json_mode: false,
    thinking: None,
    stop: vec!["STOP".into()],
  }
}

pub fn sse(body: impl AsRef<str>) -> String {
  body
    .as_ref()
    .lines()
    .map(|line| format!("data: {line}\n\n"))
    .collect::<Vec<_>>()
    .join("")
}
