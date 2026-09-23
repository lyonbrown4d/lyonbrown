mod common;

use agent_core::StopReason;
use agent_model::{ModelEvent, ModelProvider};
use futures::StreamExt;
use provider_anthropic::AnthropicProvider;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{capabilities, request, sse};

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

#[tokio::test]
async fn accumulates_tool_use_input_json_deltas_into_tool_call() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":10}}}"#
                .to_string()
                + "\n"
                + r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"forecast"}}"#
                + "\n"
                + r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"cit"}}"#
                + "\n"
                + r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"y\":\"Lyon\"}"}}"#
                + "\n"
                + r#"{"type":"content_block_stop","index":0}"#
                + "\n"
                + r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}"#
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

  let ModelEvent::ToolCall(call) = &events[0] else {
    panic!("expected tool call, got {:?}", events[0]);
  };
  assert_eq!(call.name, "forecast");
  assert_eq!(call.arguments, json!({"city": "Lyon"}));
  assert_eq!(
    events[1],
    ModelEvent::Usage(agent_core::TokenUsage {
      input: 10,
      output: 5,
      cache_read: 0,
    })
  );
  assert_eq!(
    events[2],
    ModelEvent::Done {
      stop_reason: StopReason::ToolUse
    }
  );
}
