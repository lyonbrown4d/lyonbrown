mod support;

use axum::{Json, Router, http::header, routing::post as route_post};
use gateway_core::{ProviderEndpoint, Secret};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
use support::{Audit, config, gateway, post, serve};

const WITH_USAGE: &str = r#"{"id":"known","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":5},"completion_tokens_details":{"reasoning_tokens":3}}}"#;
const WITHOUT_USAGE: &str = r#"{"id":"unknown","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#;

fn endpoint_config(base: &str) -> gateway_core::GatewayConfig {
  let mut settings = config(base);
  settings.endpoints.push(ProviderEndpoint {
    id: "usage-endpoint".into(),
    provider_id: "usage-provider".into(),
    api_base: format!("{base}/v1"),
    api_key: Some(Secret::new("upstream-secret")),
    api_key_env: None,
    enabled: true,
    priority: 0,
    max_concurrent: 4,
    model_ids: BTreeSet::from(["public-model".into()]),
  });
  settings
}

async fn reply(Json(request): Json<Value>) -> impl axum::response::IntoResponse {
  let known = request["messages"][0]["content"] == "known usage";
  (
    [(header::CONTENT_TYPE, "application/json")],
    if known { WITH_USAGE } else { WITHOUT_USAGE },
  )
}

#[tokio::test]
async fn observations_count_known_and_unknown_usage_without_fabricating_zeros() {
  let upstream = serve(Router::new().route("/v1/chat/completions", route_post(reply))).await;
  let (gateway, server) =
    gateway(endpoint_config(&upstream.base), Arc::new(Audit::default())).await;

  let mut known = support::completion("public-model");
  known["messages"][0]["content"] = json!("known usage");
  let response = post(&server.base, known)
    .send()
    .await
    .expect("known usage response");
  assert_eq!(response.text().await.expect("known body"), WITH_USAGE);

  let mut unknown = support::completion("public-model");
  unknown["messages"][0]["content"] = json!("usage absent");
  let response = post(&server.base, unknown)
    .send()
    .await
    .expect("missing usage response");
  assert_eq!(
    response.text().await.expect("missing usage body"),
    WITHOUT_USAGE
  );

  let observations = gateway.observations();
  assert_eq!(observations.requests_with_usage, 1);
  assert_eq!(observations.requests_without_usage, 1);
  assert_eq!(observations.input_tokens, 12);
  assert_eq!(observations.output_tokens, 7);
  assert_eq!(observations.cached_input_tokens, 5);
  assert_eq!(observations.reasoning_tokens, 3);
}
