mod common;

use std::sync::Arc;

use agent_core::SecretRef;
use agent_model::ModelProvider;
use agent_secret::{SecretBroker, SecretError};
use async_trait::async_trait;
use common::{capabilities, request, sse};
use provider_openai::OpenAiProvider;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct TestBroker;

#[async_trait]
impl SecretBroker for TestBroker {
  async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError> {
    assert_eq!(reference.parts(), ("env", "OPENAI_API_KEY"));
    Ok("test-key".into())
  }
}

#[tokio::test]
async fn credential_reference_is_resolved_when_request_is_sent() {
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .and(path("/chat/completions"))
    .and(header("authorization", "Bearer test-key"))
    .respond_with(ResponseTemplate::new(200).set_body_string(sse("[DONE]")))
    .mount(&server)
    .await;
  let provider = OpenAiProvider::with_secret_ref(
    Arc::new(TestBroker),
    SecretRef::env("OPENAI_API_KEY"),
    server.uri(),
    vec![capabilities("gpt-4o")],
  )
  .unwrap();
  provider.stream(request("gpt-4o")).await.unwrap();
}
