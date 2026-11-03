use axum::{
  Json, Router,
  body::{Body, to_bytes},
  http::{Request, StatusCode, header},
  routing::post,
};
use gateway_core::{
  AuditStatus, Gateway, GatewayConfig, Limits, ModelConfig, ProviderEndpoint, Secret,
};
use gateway_storage::GatewayStorage;
use std::{collections::BTreeSet, sync::Arc};

#[tokio::test]
async fn rejected_account_and_successful_retry_are_persisted_under_one_request() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("mock listener");
  let address = listener.local_addr().expect("address");
  let server = tokio::spawn(async move {
    axum::serve(listener, Router::new()
      .route("/first/chat/completions", post(|| async { StatusCode::TOO_MANY_REQUESTS }))
      .route("/second/chat/completions", post(|| async { Json(serde_json::json!({"id":"ok","object":"chat.completion","model":"actual","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]})) })))
      .await.expect("serve");
  });
  let directory = tempfile::tempdir().expect("directory");
  let store = Arc::new(
    GatewayStorage::open(directory.path().join("gateway.sqlite"))
      .await
      .expect("store"),
  );
  let token = "test-token-long-enough-for-gateway-auth";
  store.bootstrap_access_token(token).await.expect("token");
  let model = ModelConfig {
    id: "test".into(),
    upstream_model: "actual".into(),
    api_base: format!("http://{address}/first"),
    api_key: None,
    api_key_env: None,
    tier: decision_core::ModelTier::Balanced,
    local_inference: true,
    capabilities: BTreeSet::from(["text".into()]),
    context_tokens: Some(65536),
  };
  let mut alternate = model.clone();
  alternate.id = "alternate".into();
  let endpoint = |id: &str, path: &str, priority, model_id: &str| ProviderEndpoint {
    id: id.into(),
    provider_id: "mock".into(),
    api_base: format!("http://{address}/{path}"),
    api_key: None,
    api_key_env: None,
    enabled: true,
    priority,
    max_concurrent: 1,
    model_ids: BTreeSet::from([model_id.into()]),
  };
  let config = GatewayConfig {
    token: Secret::new(token),
    models: vec![model, alternate],
    endpoints: vec![
      endpoint("first", "first", 0, "test"),
      endpoint("second", "second", 1, "alternate"),
    ],
    groups: vec![gateway_core::ModelGroup {
      id: "group".into(),
      models: vec!["test".into(), "alternate".into()],
    }],
    limits: Limits::default(),
    constraints: Default::default(),
  };
  let engine = Arc::new(
    decision_core::DecisionEngine::new(decision_core::Policy::default(), vec![]).expect("engine"),
  );
  let gateway =
    Gateway::new_with_access(config, engine, store.clone(), store.clone()).expect("gateway");
  let request = Request::post("/v1/chat/completions")
    .header(header::CONTENT_TYPE, "application/json")
    .header(header::AUTHORIZATION, format!("Bearer {token}"))
    .body(Body::from(
      r#"{"model":"group","messages":[{"role":"user","content":"hello"}],"max_tokens":16}"#,
    ))
    .expect("request");
  let response = gateway.invoke(request).await;
  assert_eq!(response.status(), StatusCode::OK);
  let _ = to_bytes(response.into_body(), 8192).await.expect("body");
  let requests = store.list_audits(10, 0).await.expect("audits").records;
  assert_eq!(requests.len(), 1);
  assert!(matches!(requests[0].status, AuditStatus::Succeeded));
  assert_eq!(requests[0].model, "alternate");
  let attempts = store
    .list_attempts(&requests[0].request_id)
    .await
    .expect("attempts");
  assert_eq!(attempts.len(), 2);
  assert_eq!(attempts[0].endpoint.as_deref(), Some("first"));
  assert_eq!(attempts[0].upstream_status, Some(429));
  assert!(matches!(attempts[0].status, AuditStatus::Failed));
  assert_eq!(attempts[1].endpoint.as_deref(), Some("second"));
  assert!(matches!(attempts[1].status, AuditStatus::Succeeded));
  server.abort();
}
