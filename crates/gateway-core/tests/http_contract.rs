mod support;
use gateway_core::{AuditStatus, Secret};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
use support::*;

#[tokio::test]
async fn explicit_and_auto_route_rewrite_and_audit_real_transfer() {
  let (capture, upstream) = mock_json().await;
  let audit = Arc::new(Audit::default());
  let (gateway, server) = gateway(config(&upstream.base), audit.clone()).await;
  for model in ["public-model", "router/auto"] {
    let response = post(&server.base, completion(model))
      .send()
      .await
      .expect("response");
    assert_eq!(response.status(), 200);
    assert!(response.headers().contains_key("x-request-id"));
    let body: Value = response.json().await.expect("JSON");
    assert_eq!(body["usage"]["total_tokens"], 5);
  }
  assert_eq!(capture.calls.load(Ordering::SeqCst), 2);
  assert_eq!(gateway.observations().completions_succeeded, 2);
  assert_eq!(gateway.observations().http_requests, 2);
  for body in capture.bodies.lock().expect("capture lock").iter() {
    assert_eq!(body["model"], "actual-upstream-model");
    assert!(body.get("routing").is_none());
  }
  let records = audit.records.lock().expect("audit lock");
  assert_eq!(records.len(), 4);
  assert!(matches!(records[0].status, AuditStatus::Pending));
  assert!(matches!(records[1].status, AuditStatus::Succeeded));
  assert_eq!(records[0].request_id, records[1].request_id);
}
#[tokio::test]
async fn auth_and_constraints_never_reach_upstream() {
  let (capture, upstream) = mock_json().await;
  let mut settings = config(&upstream.base);
  settings.models[0].local_inference = false;
  let (_, server) = gateway(settings, Arc::new(Audit::default())).await;
  let response = client()
    .post(format!("{}/v1/chat/completions", server.base))
    .json(&completion("public-model"))
    .send()
    .await
    .expect("unauthorized");
  assert_eq!(response.status(), 401);
  let response = post(&server.base, completion("misspelled-model"))
    .send()
    .await
    .expect("unknown");
  assert_eq!(response.status(), 404);
  let mut body = completion("router/auto");
  body["routing"] = json!({"local_only":true});
  assert_eq!(
    post(&server.base, body)
      .send()
      .await
      .expect("local rejection")
      .status(),
    404
  );
  let mut body = completion("public-model");
  body["routing"] = json!({"allowed_tiers":["advanced"]});
  assert_eq!(
    post(&server.base, body)
      .send()
      .await
      .expect("tier rejection")
      .status(),
    404
  );
  assert_eq!(capture.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn invalid_media_context_json_and_size_are_rejected() {
  let (capture, upstream) = mock_json().await;
  let mut settings = config(&upstream.base);
  settings.limits.max_body_bytes = 1024;
  settings.models[0].context_tokens = Some(1024);
  let (_, server) = gateway(settings, Arc::new(Audit::default())).await;
  let mut image = completion("public-model");
  image["messages"][0]["content"] =
    json!([{"type":"image_url","image_url":{"url":"https://invalid.example/image"}}]);
  assert_eq!(
    post(&server.base, image)
      .send()
      .await
      .expect("media")
      .status(),
    400
  );
  let mut context = completion("public-model");
  context["max_tokens"] = json!(1024);
  assert_eq!(
    post(&server.base, context)
      .send()
      .await
      .expect("context")
      .status(),
    404
  );
  let malformed = client()
    .post(format!("{}/v1/chat/completions", server.base))
    .bearer_auth(TOKEN)
    .header("content-type", "application/json")
    .body("{")
    .send()
    .await
    .expect("malformed");
  assert_eq!(malformed.status(), 400);
  let oversized = client()
    .post(format!("{}/v1/chat/completions", server.base))
    .bearer_auth(TOKEN)
    .header("content-type", "application/json")
    .body("x".repeat(2048))
    .send()
    .await
    .expect("oversized");
  assert_eq!(oversized.status(), 413);
  assert_eq!(capture.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn failed_intent_stops_side_effect_and_readiness_can_drain() {
  let (capture, upstream) = mock_json().await;
  let (gateway, server) = gateway(
    config(&upstream.base),
    Arc::new(Audit {
      fail: true,
      ..Default::default()
    }),
  )
  .await;
  assert_eq!(
    post(&server.base, completion("public-model"))
      .send()
      .await
      .expect("audit rejected")
      .status(),
    503
  );
  assert_eq!(capture.calls.load(Ordering::SeqCst), 0);
  gateway.set_ready(false);
  assert_eq!(
    client()
      .get(format!("{}/health/ready", server.base))
      .send()
      .await
      .expect("ready")
      .status(),
    503
  );
  assert_eq!(
    client()
      .get(format!("{}/health/live", server.base))
      .send()
      .await
      .expect("live")
      .status(),
    200
  );
}
#[tokio::test]
async fn swagger_schema_is_public_and_model_list_hides_secrets() {
  let (_, upstream) = mock_json().await;
  let (_, server) = gateway(config(&upstream.base), Arc::new(Audit::default())).await;
  let initializer = client()
    .get(format!("{}/swagger-ui/swagger-initializer.js", server.base))
    .send()
    .await
    .expect("local Swagger initializer")
    .text()
    .await
    .expect("initializer body");
  assert!(initializer.contains("\"validatorUrl\": \"none\""));
  assert!(initializer.contains("\"queryConfigEnabled\": false"));
  assert!(initializer.contains("\"persistAuthorization\": false"));
  let url = format!("{}/api-doc/openapi.json", server.base);
  assert_eq!(
    client()
      .get(&url)
      .send()
      .await
      .expect("public schema")
      .status(),
    200
  );
  let schema: Value = client()
    .get(url)
    .bearer_auth(TOKEN)
    .send()
    .await
    .expect("schema")
    .json()
    .await
    .expect("schema JSON");
  assert_eq!(schema["info"]["title"], "Lyonbrown AI Gateway");
  assert_eq!(schema["info"]["version"], "0.1.0");
  assert!(
    schema["info"]["description"]
      .as_str()
      .unwrap()
      .contains("Authenticated text")
  );
  for path in [
    "/v1/models",
    "/v1/chat/completions",
    "/health/live",
    "/health/ready",
  ] {
    assert!(schema["paths"].get(path).is_some());
  }
  assert_eq!(schema["paths"].as_object().unwrap().len(), 4);
  for (path, method) in [("/v1/models", "get"), ("/v1/chat/completions", "post")] {
    assert_eq!(
      schema["paths"][path][method]["security"][0]["bearer"],
      json!([])
    );
  }
  assert!(schema["paths"]["/v1/models"]["post"].is_null());
  assert!(schema["paths"]["/v1/chat/completions"]["get"].is_null());
  for (method, path) in [
    ("POST", "/v1/models"),
    ("GET", "/v1/chat/completions"),
    ("POST", "/health/live"),
    ("DELETE", "/health/ready"),
  ] {
    assert_eq!(
      client()
        .request(method.parse().unwrap(), format!("{}{path}", server.base))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .status(),
      405,
      "handler methods must not be shared across distinct paths"
    );
  }
  for path in ["/health/live", "/health/ready"] {
    assert!(schema["paths"][path]["get"]["security"].is_null());
    assert_eq!(
      client()
        .get(format!("{}{path}", server.base))
        .send()
        .await
        .unwrap()
        .status(),
      200
    );
  }
  assert!(
    schema["components"]["securitySchemes"]
      .get("bearer")
      .is_some()
  );
  assert!(
    schema["paths"]["/v1/chat/completions"]["post"]["responses"]
      .get("413")
      .is_some()
  );
  let response = client()
    .get(format!("{}/v1/models", server.base))
    .bearer_auth(TOKEN)
    .send()
    .await
    .expect("models");
  let text = response.text().await.expect("model body");
  assert!(text.contains("public-model"));
  assert!(!text.contains("upstream-secret") && !text.contains("127.0.0.1"));
  assert!(!format!("{:?}", Secret::new("hidden-secret")).contains("hidden-secret"));
}
#[tokio::test]
async fn loopback_cloud_proxy_is_not_certified_local_and_local_url_is_validated() {
  let (_, upstream) = mock_json().await;
  let mut model = support::model(&upstream.base);
  model.api_base = "https://example.com/v1".into();
  assert!(model.validate().is_err());
  model.local_inference = false;
  assert!(model.validate().is_ok());
  model.api_base = "https://secret@example.com/v1".into();
  assert!(model.validate().is_err());
  assert!(!format!("{model:?}").contains("secret@example.com"));
}

#[tokio::test]
async fn function_tool_transcript_is_forwarded_and_capability_is_required() {
  let (capture, upstream) = mock_json().await;
  let (_, server) = gateway(config(&upstream.base), Arc::new(Audit::default())).await;
  let mut body = completion("public-model");
  body["messages"] = json!([
    {"role":"user","content":"weather"},
    {"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}}]},
    {"role":"tool","tool_call_id":"call_1","content":"sunny"}
  ]);
  let response = post(&server.base, body.clone())
    .send()
    .await
    .expect("tool transcript");
  assert_eq!(response.status(), 200);
  assert_eq!(
    capture.bodies.lock().expect("capture")[0]["messages"],
    body["messages"]
  );
  let mut settings = config(&upstream.base);
  settings.models[0].capabilities.remove("tools");
  let (_, server) = gateway(settings, Arc::new(Audit::default())).await;
  assert_eq!(
    post(&server.base, body)
      .send()
      .await
      .expect("capability rejection")
      .status(),
    404
  );
  assert_eq!(capture.calls.load(Ordering::SeqCst), 1);
}
