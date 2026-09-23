mod common;

use std::sync::Arc;

use agent_model::ModelProvider;
use provider_anthropic::AnthropicProvider;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{capabilities, request, sse};

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
