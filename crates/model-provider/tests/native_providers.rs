use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use model_provider::{ModelConfig, classify_json, list_models};
use serde_json::json;
use tokio_util::sync::CancellationToken;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn config(provider: &str, base: &str) -> ModelConfig {
  ModelConfig {
    provider: provider.into(),
    model: "fixture-model".into(),
    api_key: "explicit-test-key".into(),
    base_url: Some(base.into()),
    system_prompt: None,
    context_window: 4096,
    max_output_tokens: 128,
    supports_images: false,
  }
}
#[tokio::test]
async fn native_registry_adapters_execute_using_explicit_credentials_and_routes() -> TestResult {
  for (provider, protocol) in [
    ("openai-responses", StandardProtocol::Responses),
    ("openai-completions", StandardProtocol::ChatCompletions),
    ("anthropic", StandardProtocol::Anthropic),
    ("openrouter", StandardProtocol::ChatCompletions),
    ("groq", StandardProtocol::ChatCompletions),
    ("mistral", StandardProtocol::ChatCompletions),
    ("deepseek", StandardProtocol::ChatCompletions),
  ] {
    let server = MockModelServer::start(vec![text_reply(protocol, "{\"accepted\":true}")]).await?;
    let base = format!("{}/v1", server.base_url());
    let result = classify_json(
      &config(provider, &base),
      "Return JSON",
      CancellationToken::new(),
    )
    .await?;
    assert_eq!(result, "{\"accepted\":true}", "{provider}");
    let requests = server.requests().await;
    assert_eq!(requests.len(), 1, "{provider}");
    assert_eq!(requests[0].path, protocol.path(), "{provider}");
    if provider == "anthropic" {
      assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some("explicit-test-key")
      );
    } else {
      assert_eq!(
        requests[0].headers.get("authorization").map(String::as_str),
        Some("Bearer explicit-test-key")
      );
    }
  }
  let server = MockModelServer::start(vec![ScriptedReply::sse(vec![json!({
    "candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"{\"accepted\":true}"}]},"finishReason":"STOP"}],
    "usageMetadata":{"promptTokenCount":2,"candidatesTokenCount":3,"totalTokenCount":5}
  })])]).await?;
  let result = classify_json(
    &config("gemini", server.base_url()),
    "Return JSON",
    CancellationToken::new(),
  )
  .await?;
  assert_eq!(result, "{\"accepted\":true}");
  let requests = server.requests().await;
  assert_eq!(requests.len(), 1);
  assert!(
    requests[0]
      .path
      .starts_with("/v1beta/models/fixture-model:streamGenerateContent")
  );
  Ok(())
}
#[tokio::test]
async fn framework_model_listing_handles_native_shapes_and_redacts_failure_content() -> TestResult {
  for provider in [
    "openai-responses",
    "openrouter",
    "groq",
    "mistral",
    "deepseek",
    "anthropic",
    "gemini",
  ] {
    let body = if provider == "gemini" {
      json!({"models":[{"name":"models/fixture-model","displayName":"Fixture"}]})
    } else if provider == "anthropic" {
      json!({"data":[{"id":"fixture-model","type":"model","display_name":"Fixture","created_at":"2025-01-01T00:00:00Z"}],"has_more":false})
    } else {
      json!({"object":"list","data":[{"id":"fixture-model","object":"model","created":0,"owned_by":"fixture"}]})
    };
    let server = MockModelServer::start(vec![ScriptedReply::status(200, body)]).await?;
    let base = if provider == "gemini" {
      server.base_url().to_owned()
    } else {
      format!("{}/v1", server.base_url())
    };
    assert_eq!(
      list_models(provider, "explicit-test-key", Some(&base)).await?,
      vec!["fixture-model"]
    );
    assert_eq!(server.requests().await.len(), 1);
  }
  let server = MockModelServer::start(vec![ScriptedReply::status(
    401,
    json!({"error":"explicit-test-key"}),
  )])
  .await?;
  let error = list_models("gemini", "explicit-test-key", Some(server.base_url()))
    .await
    .expect_err("rejected credential");
  assert!(!error.to_string().contains("explicit-test-key"));
  Ok(())
}
