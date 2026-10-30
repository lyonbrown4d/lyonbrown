mod support;
use axum::{Router, http::header, routing::post as route_post};
use gateway_core::{AuditStatus, TokenUsage};
use serde_json::json;
use std::collections::BTreeSet;
use std::{sync::Arc, time::Duration};
use support::*;

const COMPLETION: &str = r#"{"id":"c","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":5},"completion_tokens_details":{"reasoning_tokens":3}}}"#;

fn endpoint_config(base: &str) -> gateway_core::GatewayConfig {
  let mut config = config(base);
  config.endpoints.push(gateway_core::ProviderEndpoint {
    id: "mock-endpoint".into(),
    provider_id: "mock-provider".into(),
    api_base: format!("{base}/v1"),
    api_key: Some(gateway_core::Secret::new("upstream-secret")),
    api_key_env: None,
    enabled: true,
    priority: 0,
    max_concurrent: 8,
    model_ids: BTreeSet::from(["public-model".into()]),
  });
  config
}

#[tokio::test]
async fn json_usage_is_audited_without_changing_response_bytes() {
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    route_post(|| async { ([(header::CONTENT_TYPE, "application/json")], COMPLETION) }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(endpoint_config(&upstream.base), audit.clone()).await;
  let response = post(&server.base, completion("public-model"))
    .send()
    .await
    .expect("response");
  assert_eq!(response.text().await.expect("body"), COMPLETION);
  let record = audit
    .records
    .lock()
    .expect("audit")
    .last()
    .cloned()
    .expect("record");
  assert_eq!(
    record.usage,
    Some(TokenUsage {
      input_tokens: 12,
      output_tokens: 7,
      cached_input_tokens: 5,
      reasoning_tokens: 3,
    })
  );
  assert!(matches!(record.status, AuditStatus::Succeeded));
}

#[tokio::test]
async fn sse_usage_split_across_chunks_is_audited_and_bytes_are_preserved() {
  let first = "data: {\"id\":\"c\",\"choices\":[{\"index\":0,\"delta\":{}}],\"usage\":null}\n\n";
  let second_a = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":";
  let second_b = "9,\"completion_tokens\":4,\"prompt_tokens_details\":null,\"completion_tokens_details\":{\"reasoning_tokens\":null}}}\n\n";
  let tail = "data: [DONE]\n\n";
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    route_post(move || async move {
      let chunks = [first, second_a, second_b, tail];
      let body = axum::body::Body::from_stream(async_stream::stream! {
        for chunk in chunks {
          tokio::time::sleep(Duration::from_millis(2)).await;
          yield Ok::<_, std::io::Error>(chunk);
        }
      });
      ([(header::CONTENT_TYPE, "text/event-stream")], body)
    }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(endpoint_config(&upstream.base), audit.clone()).await;
  let mut request = completion("public-model");
  request["stream"] = json!(true);
  let response = post(&server.base, request).send().await.expect("response");
  assert_eq!(
    response.text().await.expect("SSE body"),
    format!("{first}{second_a}{second_b}{tail}")
  );
  let record = audit
    .records
    .lock()
    .expect("audit")
    .last()
    .cloned()
    .expect("record");
  assert_eq!(
    record.usage,
    Some(TokenUsage {
      input_tokens: 9,
      output_tokens: 4,
      cached_input_tokens: 0,
      reasoning_tokens: 0,
    })
  );
  assert!(matches!(record.status, AuditStatus::Succeeded));
}

#[tokio::test]
async fn missing_usage_stays_unknown_and_overflow_is_not_reported_as_zero() {
  for no_usage in [
    r#"{"id":"c","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#,
    r#"{"id":"c","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":null}"#,
  ] {
    let upstream = serve(Router::new().route(
      "/v1/chat/completions",
      route_post(move || async move { ([(header::CONTENT_TYPE, "application/json")], no_usage) }),
    ))
    .await;
    let audit = Arc::new(Audit::default());
    let (_, server) = gateway(endpoint_config(&upstream.base), audit.clone()).await;
    let response = post(&server.base, completion("public-model"))
      .send()
      .await
      .expect("response");
    assert_eq!(response.text().await.expect("body"), no_usage);
    assert_eq!(
      audit
        .records
        .lock()
        .expect("audit")
        .last()
        .expect("record")
        .usage,
      None
    );
  }

  let overflow = r#"{"id":"c","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":18446744073709551616,"completion_tokens":1}}"#;
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    route_post(move || async move { ([(header::CONTENT_TYPE, "application/json")], overflow) }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(endpoint_config(&upstream.base), audit.clone()).await;
  assert_eq!(
    post(&server.base, completion("public-model"))
      .send()
      .await
      .expect("response")
      .status(),
    502
  );
  let record = audit
    .records
    .lock()
    .expect("audit")
    .last()
    .cloned()
    .expect("record");
  assert_eq!(record.usage, None);
  assert!(matches!(record.status, AuditStatus::Uncertain));
}

#[tokio::test]
async fn inconsistent_usage_totals_are_uncertain() {
  for usage in [
    r#"{"prompt_tokens":4,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":5}}"#,
    r#"{"prompt_tokens":4,"completion_tokens":2,"completion_tokens_details":{"reasoning_tokens":3}}"#,
    r#"{"prompt_tokens":18446744073709551615,"completion_tokens":1}"#,
  ] {
    let body = format!(
      r#"{{"id":"c","object":"chat.completion","choices":[{{"index":0,"message":{{"role":"assistant","content":"ok"}},"finish_reason":"stop"}}],"usage":{usage}}}"#
    );
    let upstream = serve(Router::new().route(
      "/v1/chat/completions",
      route_post(move || {
        let body = body.clone();
        async move { ([(header::CONTENT_TYPE, "application/json")], body) }
      }),
    ))
    .await;
    let audit = Arc::new(Audit::default());
    let (_, server) = gateway(endpoint_config(&upstream.base), audit.clone()).await;
    assert_eq!(
      post(&server.base, completion("public-model"))
        .send()
        .await
        .expect("response")
        .status(),
      502
    );
    let record = audit
      .records
      .lock()
      .expect("audit")
      .last()
      .cloned()
      .expect("record");
    assert_eq!(record.usage, None);
    assert!(matches!(record.status, AuditStatus::Uncertain));
  }
}
