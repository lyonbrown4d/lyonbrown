use super::*;

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

#[tokio::test]
async fn returns_http_error_with_body_for_auth_failure() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/messages"))
    .respond_with(ResponseTemplate::new(401).set_body_string(
      r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
    ))
    .mount(&server)
    .await;

  let provider = AnthropicProvider::with_base_url(
    "sk-bad",
    server.uri(),
    vec![capabilities("claude-sonnet-4-5")],
  )
  .unwrap();

  let error = provider
    .stream(request("claude-sonnet-4-5"))
    .await
    .err()
    .expect("auth failure must fail");
  match error {
    agent_model::ModelError::Http(status, body) => {
      assert_eq!(status, 401);
      assert!(body.contains("invalid x-api-key"));
    }
    other => panic!("expected Http error, got {other:?}"),
  }
}

#[tokio::test]
async fn returns_http_error_for_rate_limit() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/messages"))
    .respond_with(ResponseTemplate::new(429).set_body_string(
      r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited"}}"#,
    ))
    .mount(&server)
    .await;

  let provider = AnthropicProvider::with_base_url(
    "sk-test",
    server.uri(),
    vec![capabilities("claude-sonnet-4-5")],
  )
  .unwrap();

  let error = provider
    .stream(request("claude-sonnet-4-5"))
    .await
    .err()
    .expect("rate limit must fail");
  match error {
    agent_model::ModelError::Http(status, _) => assert_eq!(status, 429),
    other => panic!("expected Http error, got {other:?}"),
  }
}

#[tokio::test]
async fn rejects_requests_for_unconfigured_models_before_network_io() {
  let server = MockServer::start().await;
  let provider = Arc::new(
    AnthropicProvider::with_base_url(
      "sk-test",
      server.uri(),
      vec![capabilities("claude-haiku-4-5")],
    )
    .unwrap(),
  );

  let error = provider
    .stream(request("missing-model"))
    .await
    .err()
    .expect("request should fail");

  assert!(error.to_string().contains("not configured"));
  assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn endpoint_rejects_userinfo() {
  let result = AnthropicProvider::with_base_url(
    "sk-test",
    "https://user:password@example.com",
    vec![capabilities("model")],
  );

  let error = result.err().expect("userinfo must be rejected");
  assert!(error.to_string().contains("userinfo"));
}

#[test]
fn endpoint_requires_https_for_non_loopback_hosts() {
  for endpoint in ["http://example.com", "http://192.168.1.10:8080"] {
    let result = AnthropicProvider::with_base_url("sk-test", endpoint, vec![capabilities("model")]);
    let error = result
      .err()
      .expect("insecure remote endpoint must be rejected");
    assert!(error.to_string().contains("HTTPS"));
  }
}

#[test]
fn endpoint_allows_http_only_for_loopback_hosts() {
  for endpoint in [
    "http://localhost:8080",
    "http://127.42.0.1:8080",
    "https://example.com",
  ] {
    AnthropicProvider::with_base_url("sk-test", endpoint, vec![capabilities("model")]).unwrap();
  }
}
