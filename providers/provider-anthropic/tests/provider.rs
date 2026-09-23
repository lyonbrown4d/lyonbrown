use std::sync::Arc;

use agent_core::{
  Locality, ModelCapabilities, ModelTier, RiskLevel, StopReason, ToolDefinition, ToolSource,
};
use agent_model::{ChatMessage, ModelEvent, ModelProvider, ModelRequest, Role};
use futures::StreamExt;
use provider_anthropic::AnthropicProvider;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn capabilities(model: &str) -> ModelCapabilities {
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

fn request(model: &str) -> ModelRequest {
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

fn sse(body: impl AsRef<str>) -> String {
  body
    .as_ref()
    .lines()
    .map(|line| format!("data: {line}\n\n"))
    .collect::<Vec<_>>()
    .join("")
}

#[tokio::test]
async fn sends_messages_request_with_system_tools_and_max_tokens() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/messages"))
    .and(header("x-api-key", "sk-test"))
    .and(header("anthropic-version", "2023-06-01"))
    .and(body_json(json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 2048,
        "system": "Be concise",
        "messages": [{"role": "user", "content": "weather"}],
        "tools": [{
            "name": "forecast",
            "description": "Get a forecast",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        }],
        "stream": true,
        "temperature": 0.2,
        "stop_sequences": ["STOP"]
    })))
    .respond_with(ResponseTemplate::new(200).set_body_string(sse(r#"{"type":"message_stop"}"#)))
    .mount(&server)
    .await;

  let provider = AnthropicProvider::with_base_url(
    "sk-test",
    server.uri(),
    vec![capabilities("claude-sonnet-4-5")],
  )
  .unwrap();
  provider.stream(request("claude-sonnet-4-5")).await.unwrap();
}

#[tokio::test]
async fn parses_sse_text_deltas_usage_and_done() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":12,"cache_read_input_tokens":4}}}"#
                .to_string()
                + "\n"
                + r#"{"type":"content_block_start","index":0,"content_block":{"type":"text"}}"#
                + "\n"
                + r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me "}}"#
                + "\n"
                + r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"check."}}"#
                + "\n"
                + r#"{"type":"content_block_stop","index":0}"#
                + "\n"
                + r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}"#
                + "\n"
                + r#"{"type":"message_stop"}"#,
        )))
        .mount(&server)
        .await;

  let provider = AnthropicProvider::with_base_url(
    "sk-test",
    server.uri(),
    vec![capabilities("claude-sonnet-4-5")],
  )
  .unwrap();
  let events = provider
    .stream(request("claude-sonnet-4-5"))
    .await
    .unwrap()
    .events
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();

  assert_eq!(events[0], ModelEvent::Delta("Let me ".into()));
  assert_eq!(events[1], ModelEvent::Delta("check.".into()));
  assert_eq!(
    events[2],
    ModelEvent::Usage(agent_core::TokenUsage {
      input: 12,
      output: 7,
      cache_read: 4,
    })
  );
  assert_eq!(
    events[3],
    ModelEvent::Done {
      stop_reason: StopReason::EndTurn
    }
  );
}

#[path = "provider/extended.rs"]
mod extended;
