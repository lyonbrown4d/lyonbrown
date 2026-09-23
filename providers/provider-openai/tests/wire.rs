mod common;

use std::sync::Arc;

use agent_model::ModelProvider;
use provider_openai::OpenAiProvider;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use common::{capabilities, request, sse};

#[tokio::test]
async fn sends_completions_request_with_system_tools_and_stream_options() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .and(header("authorization", "Bearer sk-test"))
    .and(body_json(json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "Be concise"},
            {"role": "user", "content": "weather"}
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "forecast",
                "description": "Get a forecast",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }
        }],
        "stream": true,
        "stream_options": {"include_usage": true},
        "max_tokens": 2048,
        "temperature": 0.2
    })))
    .respond_with(ResponseTemplate::new(200).set_body_string(sse(
      r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}
{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":1}}
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
  provider.stream(request("gpt-4o")).await.unwrap();
}

#[tokio::test]
async fn rejects_requests_for_unconfigured_models_before_network_io() {
  let server = MockServer::start().await;
  let provider = Arc::new(
    OpenAiProvider::with_base_url(
      "sk-test",
      format!("{}/v1", server.uri()),
      vec![capabilities("gpt-4o-mini")],
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
  let result = OpenAiProvider::with_base_url(
    "sk-test",
    "https://user:password@example.com/v1",
    vec![capabilities("model")],
  );

  let error = result.err().expect("userinfo must be rejected");
  assert!(error.to_string().contains("userinfo"));
}

#[test]
fn endpoint_requires_https_for_non_loopback_hosts() {
  for endpoint in ["http://example.com/v1", "http://192.168.1.10:8080/v1"] {
    let result = OpenAiProvider::with_base_url("sk-test", endpoint, vec![capabilities("model")]);
    let error = result
      .err()
      .expect("insecure remote endpoint must be rejected");
    assert!(error.to_string().contains("HTTPS"));
  }
}

#[test]
fn endpoint_allows_http_only_for_loopback_hosts() {
  for endpoint in [
    "http://localhost:8080/v1",
    "http://127.42.0.1:8080/v1",
    "https://example.com/v1",
  ] {
    OpenAiProvider::with_base_url("sk-test", endpoint, vec![capabilities("model")]).unwrap();
  }
}
