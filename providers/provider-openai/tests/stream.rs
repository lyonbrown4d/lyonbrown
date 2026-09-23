mod common;

use agent_core::StopReason;
use agent_model::{ModelEvent, ModelProvider};
use futures::StreamExt;
use provider_openai::OpenAiProvider;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{capabilities, request, sse};

#[tokio::test]
async fn accumulates_multi_segment_tool_call_deltas_by_index() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse(
            r#"{"choices":[{"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","function":{"name":"forecast","arguments":"{\"cit"}}]}}]}
{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"y\":\"Lyon\"}"}}]}}]}
{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_2","function":{"name":"forecast","arguments":"{\"city\":"}}]}}]}
{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":" \"Paris\"}"}}]}}]}
{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}
{"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":9}}
[DONE]"#,
        )))
        .mount(&server)
        .await;

  let provider = OpenAiProvider::with_base_url(
    "sk-test",
    format!("{}/v1", server.uri()),
    vec![capabilities("gpt-4o")],
  )
  .unwrap();
  let events = provider
    .stream(request("gpt-4o"))
    .await
    .unwrap()
    .events
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();

  assert_eq!(events.len(), 4);
  let ModelEvent::ToolCall(first) = &events[0] else {
    panic!("expected first tool call, got {:?}", events[0]);
  };
  assert_eq!(first.name, "forecast");
  assert_eq!(first.arguments, json!({"city": "Lyon"}));
  let ModelEvent::ToolCall(second) = &events[1] else {
    panic!("expected second tool call, got {:?}", events[1]);
  };
  assert_eq!(second.name, "forecast");
  assert_eq!(second.arguments, json!({"city": "Paris"}));
  assert_eq!(
    events[2],
    ModelEvent::Usage(agent_core::TokenUsage {
      input: 11,
      output: 9,
      cache_read: 0,
    })
  );
  assert_eq!(
    events[3],
    ModelEvent::Done {
      stop_reason: StopReason::ToolUse
    }
  );
}

#[tokio::test]
async fn parses_text_deltas_usage_and_done() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .respond_with(ResponseTemplate::new(200).set_body_string(sse(
      r#"{"choices":[{"delta":{"role":"assistant","content":"Let me "}}]}
{"choices":[{"delta":{"content":"check."}}]}
{"choices":[{"delta":{},"finish_reason":"stop"}]}
{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7}}
[DONE]"#,
    )))
    .mount(&server)
    .await;

  let provider = OpenAiProvider::with_base_url(
    "sk-test",
    format!("{}/v1", server.uri()),
    vec![capabilities("gpt-4o")],
  )
  .unwrap();
  let events = provider
    .stream(request("gpt-4o"))
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
      cache_read: 0,
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
async fn returns_http_error_with_body_for_auth_failure() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(401).set_body_string(
                r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error","code":"invalid_api_key"}}"#,
            ),
        )
        .mount(&server)
        .await;

  let provider = OpenAiProvider::with_base_url(
    "sk-bad",
    format!("{}/v1", server.uri()),
    vec![capabilities("gpt-4o")],
  )
  .unwrap();

  let error = provider
    .stream(request("gpt-4o"))
    .await
    .err()
    .expect("auth failure must fail");
  match error {
    agent_model::ModelError::Http(status, body) => {
      assert_eq!(status, 401);
      assert!(body.contains("Incorrect API key"));
    }
    other => panic!("expected Http error, got {other:?}"),
  }
}

#[tokio::test]
async fn returns_http_error_for_rate_limit() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .respond_with(
      ResponseTemplate::new(429)
        .set_body_string(r#"{"error":{"message":"Rate limit reached","type":"requests"}}"#),
    )
    .mount(&server)
    .await;

  let provider = OpenAiProvider::with_base_url(
    "sk-test",
    format!("{}/v1", server.uri()),
    vec![capabilities("gpt-4o")],
  )
  .unwrap();

  let error = provider
    .stream(request("gpt-4o"))
    .await
    .err()
    .expect("rate limit must fail");
  match error {
    agent_model::ModelError::Http(status, _) => assert_eq!(status, 429),
    other => panic!("expected Http error, got {other:?}"),
  }
}

#[tokio::test]
async fn rejects_stream_that_ends_without_done_marker() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .respond_with(ResponseTemplate::new(200).set_body_string(sse(
      r#"{"choices":[{"delta":{"content":"partial"},"finish_reason":"stop"}]}"#,
    )))
    .mount(&server)
    .await;
  let provider = OpenAiProvider::with_base_url(
    "sk-test",
    format!("{}/v1", server.uri()),
    vec![capabilities("gpt-4o")],
  )
  .unwrap();
  let events = provider
    .stream(request("gpt-4o"))
    .await
    .unwrap()
    .events
    .collect::<Vec<_>>()
    .await;
  assert!(events.iter().any(|event| matches!(event,
    Err(agent_model::ModelError::Protocol(message)) if message.contains("[DONE]"))));
  assert!(
    !events
      .iter()
      .any(|event| matches!(event, Ok(ModelEvent::Done { .. })))
  );
}

#[tokio::test]
async fn parses_multiline_sse_data_with_crlf() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .respond_with(ResponseTemplate::new(200).set_body_string(concat!(
      "data: {\"choices\":[{\"delta\":{\"content\":\r\n",
      "data: \"joined\"}}]}\r\n\r\n",
      "data: [DONE]\r\n\r\n"
    )))
    .mount(&server)
    .await;
  let provider = OpenAiProvider::with_base_url(
    "sk-test",
    format!("{}/v1", server.uri()),
    vec![capabilities("gpt-4o")],
  )
  .unwrap();

  let events = provider
    .stream(request("gpt-4o"))
    .await
    .unwrap()
    .events
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();

  assert_eq!(events[0], ModelEvent::Delta("joined".into()));
}
