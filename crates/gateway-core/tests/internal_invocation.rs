mod support;

use axum::{
  body::{Body, to_bytes},
  http::{Request, StatusCode, header},
};
use gateway_core::{AuditStatus, Gateway};
use std::sync::Arc;
use support::{Audit, TOKEN, completion, config};

struct ConfiguredAccess;
#[async_trait::async_trait]
impl gateway_core::AccessControl for ConfiguredAccess {
  async fn authorize(&self, bearer: &str) -> Result<bool, gateway_core::AccessError> {
    Ok(bearer == TOKEN)
  }
}

#[tokio::test]
async fn shared_credential_updates_protect_both_entry_points_without_rebuilding_gateway() {
  use tower::ServiceExt;

  let (capture, upstream) = support::mock_json().await;
  let privacy = Arc::new(tokio::sync::Mutex::new(
    agent_privacy::PrivacyService::new(&[]).expect("privacy"),
  ));
  let gateway = Gateway::new_with_access_and_privacy(
    config(&upstream.base),
    Arc::new(
      decision_core::DecisionEngine::new(decision_core::Policy::default(), vec![])
        .expect("decision"),
    ),
    Arc::new(Audit::default()),
    Arc::new(ConfiguredAccess),
    privacy.clone(),
  )
  .expect("gateway");
  let credential = "opaqueOtherCredential628142";
  privacy
    .lock()
    .await
    .register_known_secret(&secrecy::SecretString::from(credential))
    .expect("register");
  let request = || {
    let mut body = completion("public-model");
    body["messages"][0]["content"] = serde_json::json!(credential);
    Request::post("/v1/chat/completions")
      .header(header::CONTENT_TYPE, "application/json")
      .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
      .body(Body::from(body.to_string()))
      .expect("request")
  };
  for response in [
    gateway.invoke(request()).await,
    gateway.router().oneshot(request()).await.expect("router"),
  ] {
    assert_eq!(response.status(), StatusCode::OK);
    let _ = to_bytes(response.into_body(), 8192).await.expect("body");
  }
  let captured = capture.bodies.lock().expect("captures");
  assert_eq!(captured.len(), 2);
  for body in captured.iter() {
    let text = body["messages"][0]["content"].as_str().expect("content");
    assert!(!text.contains(credential));
    assert!(text.starts_with("[PRIVATE:"));
  }
  assert_eq!(captured[0]["messages"], captured[1]["messages"]);
}

#[tokio::test]
async fn internal_invocation_authenticates_and_audits_the_same_pipeline() {
  let upstream = support::serve(axum::Router::new().route(
    "/v1/chat/completions",
    axum::routing::post(|| async {
      axum::Json(serde_json::json!({"id":"test","object":"chat.completion","model":"actual-upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}))
    }),
  )).await;
  let audit = Arc::new(Audit::default());
  let decision = Arc::new(
    decision_core::DecisionEngine::new(decision_core::Policy::default(), vec![]).expect("decision"),
  );
  let gateway = Gateway::new(config(&upstream.base), decision, audit.clone()).expect("gateway");
  let request = |token: &str| {
    Request::post("/v1/chat/completions")
      .header(header::CONTENT_TYPE, "application/json")
      .header(header::AUTHORIZATION, format!("Bearer {token}"))
      .body(Body::from(completion("public-model").to_string()))
      .expect("request")
  };
  assert_eq!(
    gateway.invoke(request("invalid")).await.status(),
    StatusCode::UNAUTHORIZED
  );
  let response = gateway.invoke(request(TOKEN)).await;
  assert_eq!(response.status(), StatusCode::OK);
  let body = to_bytes(response.into_body(), 8192).await.expect("body");
  assert!(
    std::str::from_utf8(&body)
      .expect("utf8")
      .contains("actual-upstream-model")
  );
  let records = audit.records.lock().expect("audit");
  assert_eq!(records.len(), 2);
  assert!(matches!(records[0].status, AuditStatus::Pending));
  assert!(matches!(records[1].status, AuditStatus::Succeeded));
}

#[tokio::test]
async fn internal_invocation_masks_sensitive_content_and_rejects_sensitive_schema() {
  let (capture, upstream) = support::mock_json().await;
  let audit = Arc::new(Audit::default());
  let decision = Arc::new(
    decision_core::DecisionEngine::new(decision_core::Policy::default(), vec![]).expect("decision"),
  );
  let gateway = Gateway::new(config(&upstream.base), decision, audit.clone()).expect("gateway");
  let request = |body: serde_json::Value| {
    Request::post("/v1/chat/completions")
      .header(header::CONTENT_TYPE, "application/json")
      .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
      .body(Body::from(body.to_string()))
      .expect("request")
  };
  let body = serde_json::json!({"model":"public-model", "messages":[{"role":"user","content":format!("alice@example.com {TOKEN} upstream-secret")}], "max_tokens": 32});
  for _ in 0..2 {
    let response = gateway.invoke(request(body.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = to_bytes(response.into_body(), 8192).await.expect("body");
  }
  {
    let captured = capture.bodies.lock().expect("captures");
    let first = captured[0]["messages"][0]["content"]
      .as_str()
      .expect("content");
    assert!(!first.contains("alice@example.com"));
    assert!(!first.contains(TOKEN));
    assert!(!first.contains("upstream-secret"));
    assert!(first.contains("[PRIVATE:"));
    assert_eq!(
      first,
      captured[1]["messages"][0]["content"]
        .as_str()
        .expect("content")
    );
  }
  let body = serde_json::json!({"model":"public-model", "messages":[{"role":"user","content":"hello"}], "max_tokens": 32, "response_format":{"type":"json_schema", "json_schema":{"name":"result", "schema":{"const":"alice@example.com"}}}});
  assert_eq!(
    gateway.invoke(request(body)).await.status(),
    StatusCode::UNPROCESSABLE_ENTITY
  );
  assert_eq!(capture.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
  assert_eq!(audit.records.lock().expect("audit").len(), 4);
}
