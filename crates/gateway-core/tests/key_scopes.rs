mod support;

use async_trait::async_trait;
use axum::http::StatusCode;
use decision_core::{DecisionEngine, ModelTier, Policy};
use gateway_core::{
  AccessControl, AccessError, AccessPrincipal, Gateway, ModelConfig, ProviderEndpoint, Secret,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
use support::{Audit, TOKEN, mock_json, post, serve};

struct ScopedAccess(AccessPrincipal);

#[async_trait]
impl AccessControl for ScopedAccess {
  async fn authorize(&self, bearer: &str) -> Result<bool, AccessError> {
    Ok(bearer == TOKEN)
  }

  async fn authenticate(&self, bearer: &str) -> Result<Option<AccessPrincipal>, AccessError> {
    Ok((bearer == TOKEN).then(|| self.0.clone()))
  }
}

fn model(id: &str, upstream_model: &str, base: &str) -> ModelConfig {
  ModelConfig {
    id: id.into(),
    upstream_model: upstream_model.into(),
    api_base: format!("{base}/v1"),
    api_key: Some(Secret::new("provider-secret")),
    api_key_env: None,
    tier: ModelTier::Balanced,
    local_inference: true,
    capabilities: BTreeSet::from(["text".into(), "tools".into()]),
    context_tokens: Some(65_536),
  }
}

#[tokio::test]
async fn scoped_key_filters_models_blocks_direct_and_auto_escalation_and_enforces_output_cap() {
  let (capture, upstream) = mock_json().await;
  let mut config = support::config(&upstream.base);
  config.models = vec![
    model("allowed-model", "allowed-upstream", &upstream.base),
    model("disallowed-model", "disallowed-upstream", &upstream.base),
  ];
  config.endpoints = vec![ProviderEndpoint {
    id: "mock-endpoint".into(),
    provider_id: "mock-provider".into(),
    api_base: format!("{}/v1", upstream.base),
    api_key: Some(Secret::new("provider-secret")),
    api_key_env: None,
    enabled: true,
    priority: 0,
    max_concurrent: 1,
    model_ids: BTreeSet::from(["allowed-model".into()]),
  }];
  let access = Arc::new(ScopedAccess(AccessPrincipal {
    key_id: "scoped-key".into(),
    project_id: "project-a".into(),
    allowed_models: BTreeSet::from(["allowed-model".into()]),
    max_output_tokens: Some(4),
  }));
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("engine"));
  let gateway =
    Gateway::new_with_access(config, engine, Arc::new(Audit::default()), access).expect("gateway");
  let server = serve(gateway.router()).await;

  let models: Value = support::client()
    .get(format!("{}/v1/models", server.base))
    .bearer_auth(TOKEN)
    .send()
    .await
    .expect("models response")
    .json()
    .await
    .expect("models JSON");
  assert_eq!(models["data"].as_array().expect("model list").len(), 1);
  assert_eq!(models["data"][0]["id"], "allowed-model");

  let direct = post(
    &server.base,
    json!({"model":"disallowed-model","messages":[{"role":"user","content":"hello"}],"max_tokens":3}),
  )
    .send()
    .await
    .expect("forbidden model response");
  assert_eq!(direct.status(), StatusCode::NOT_FOUND);
  assert_eq!(capture.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

  let auto = post(
    &server.base,
    json!({"model":"router/auto","messages":[{"role":"user","content":"hello"}],"max_tokens":3}),
  )
  .send()
  .await
  .expect("auto response");
  assert_eq!(auto.status(), StatusCode::OK);
  {
    let bodies = capture.bodies.lock().expect("captured requests");
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["model"], "allowed-upstream");
  }

  let too_many_tokens = post(
    &server.base,
    json!({"model":"allowed-model","messages":[{"role":"user","content":"hello"}],"max_tokens":5}),
  )
  .send()
  .await
  .expect("output scope response");
  assert_eq!(too_many_tokens.status(), StatusCode::FORBIDDEN);
  assert_eq!(capture.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

  let too_many_choices = post(
    &server.base,
    json!({"model":"allowed-model","messages":[{"role":"user","content":"hello"}],"max_tokens":3,"n":2}),
  )
  .send()
  .await
  .expect("aggregate output scope response");
  assert_eq!(too_many_choices.status(), StatusCode::FORBIDDEN);
  assert_eq!(capture.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
