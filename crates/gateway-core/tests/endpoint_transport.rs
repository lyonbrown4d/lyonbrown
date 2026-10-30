mod support;

use axum::{
  Json, Router,
  body::Body,
  extract::State,
  http::{HeaderMap, StatusCode, header},
  response::{IntoResponse, Response},
  routing::post as route_post,
};
use gateway_core::{AuditStatus, Gateway, ProviderEndpoint, Secret};
use serde_json::{Value, json};
use std::{
  sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};
use support::{Audit, Server, config, post, serve};

#[derive(Clone, Copy)]
enum MockBehavior {
  Success,
  RateLimited,
  PendingStream,
}

type CapturedRequest = (Option<String>, Value);
type RequestCaptures = Arc<Mutex<Vec<CapturedRequest>>>;

#[derive(Clone)]
struct MockState {
  behavior: MockBehavior,
  calls: Arc<AtomicUsize>,
  requests: RequestCaptures,
}

impl Default for MockState {
  fn default() -> Self {
    Self {
      behavior: MockBehavior::Success,
      calls: Arc::new(AtomicUsize::new(0)),
      requests: Arc::new(Mutex::new(Vec::new())),
    }
  }
}

impl MockState {
  fn with_behavior(behavior: MockBehavior) -> Self {
    Self {
      behavior,
      ..Self::default()
    }
  }
}

async fn mock_handler(
  State(state): State<MockState>,
  headers: HeaderMap,
  Json(body): Json<Value>,
) -> Response {
  state.calls.fetch_add(1, Ordering::SeqCst);
  let authorization = headers
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .map(str::to_owned);
  state
    .requests
    .lock()
    .expect("request capture lock")
    .push((authorization, body));
  match state.behavior {
    MockBehavior::Success => Json(json!({
      "id":"endpoint-completion",
      "object":"chat.completion",
      "model":"actual-upstream-model",
      "choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]
    }))
    .into_response(),
    MockBehavior::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response(),
    MockBehavior::PendingStream => {
      let stream = async_stream::stream! {
        yield Ok::<_, std::io::Error>(
          "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n",
        );
        std::future::pending::<()>().await;
      };
      Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream))
        .expect("stream response")
    }
  }
}

async fn mock(behavior: MockBehavior) -> (MockState, Server) {
  let state = MockState::with_behavior(behavior);
  let server = serve(
    Router::new()
      .route("/v1/chat/completions", route_post(mock_handler))
      .with_state(state.clone()),
  )
  .await;
  (state, server)
}

fn endpoint(
  id: &str,
  base: &str,
  key: &str,
  enabled: bool,
  priority: u32,
  max_concurrent: usize,
) -> ProviderEndpoint {
  ProviderEndpoint {
    id: id.into(),
    provider_id: "test-provider".into(),
    api_base: format!("{base}/v1"),
    api_key: Some(Secret::new(key)),
    api_key_env: None,
    enabled,
    priority,
    max_concurrent,
    model_ids: ["public-model".into()].into(),
  }
}

fn gateway(settings: gateway_core::GatewayConfig, audit: Arc<Audit>) -> Gateway {
  let engine = Arc::new(
    decision_core::DecisionEngine::new(decision_core::Policy::default(), vec![])
      .expect("decision engine"),
  );
  Gateway::new(settings, engine, audit).expect("gateway")
}

#[tokio::test]
async fn mapped_endpoint_receives_rewritten_model_and_its_credential() {
  let (legacy_capture, legacy) = mock(MockBehavior::Success).await;
  let (endpoint_capture, selected) = mock(MockBehavior::Success).await;
  let mut settings = config(&legacy.base);
  settings.models[0].upstream_model = "rewritten-upstream-model".into();
  settings.endpoints = vec![endpoint(
    "selected",
    &selected.base,
    "selected-endpoint-key",
    true,
    0,
    2,
  )];
  let server = serve(gateway(settings, Arc::new(Audit::default())).router()).await;

  let response = post(
    &server.base,
    json!({"model":"public-model","messages":[{"role":"user","content":"hello"}],"max_tokens":16}),
  )
  .send()
  .await
  .expect("completion response");
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(legacy_capture.calls.load(Ordering::SeqCst), 0);
  let requests = endpoint_capture.requests.lock().expect("captured request");
  assert_eq!(requests.len(), 1);
  assert_eq!(
    requests[0].0.as_deref(),
    Some("Bearer selected-endpoint-key")
  );
  assert_eq!(requests[0].1["model"], "rewritten-upstream-model");
}

#[tokio::test]
async fn explicit_rate_limit_cools_endpoint_for_the_next_request_without_replay() {
  let (primary_capture, primary) = mock(MockBehavior::RateLimited).await;
  let (backup_capture, backup) = mock(MockBehavior::Success).await;
  let mut settings = config(&primary.base);
  settings.endpoints = vec![
    endpoint("primary", &primary.base, "primary-key", true, 0, 2),
    endpoint("backup", &backup.base, "backup-key", true, 1, 2),
  ];
  let server = serve(gateway(settings, Arc::new(Audit::default())).router()).await;
  let body =
    json!({"model":"public-model","messages":[{"role":"user","content":"hello"}],"max_tokens":16});

  let first = post(&server.base, body.clone())
    .send()
    .await
    .expect("rate-limited response");
  assert_eq!(first.status(), StatusCode::TOO_MANY_REQUESTS);
  assert_eq!(primary_capture.calls.load(Ordering::SeqCst), 1);
  assert_eq!(backup_capture.calls.load(Ordering::SeqCst), 0);

  let second = post(&server.base, body)
    .send()
    .await
    .expect("next request response");
  assert_eq!(second.status(), StatusCode::OK);
  assert_eq!(primary_capture.calls.load(Ordering::SeqCst), 1);
  assert_eq!(backup_capture.calls.load(Ordering::SeqCst), 1);
  let requests = backup_capture.requests.lock().expect("backup request");
  assert_eq!(requests[0].0.as_deref(), Some("Bearer backup-key"));
  assert_eq!(requests[0].1["model"], "actual-upstream-model");
}

#[tokio::test]
async fn disabled_mapped_endpoint_does_not_fall_back_to_legacy_model_address() {
  let (legacy_capture, legacy) = mock(MockBehavior::Success).await;
  let (disabled_capture, disabled) = mock(MockBehavior::Success).await;
  let mut settings = config(&legacy.base);
  settings.endpoints = vec![endpoint(
    "disabled",
    &disabled.base,
    "disabled-key",
    false,
    0,
    2,
  )];
  let server = serve(gateway(settings, Arc::new(Audit::default())).router()).await;
  let response = post(
    &server.base,
    json!({"model":"public-model","messages":[{"role":"user","content":"hello"}],"max_tokens":16}),
  )
  .send()
  .await
  .expect("endpoint unavailable response");
  assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(legacy_capture.calls.load(Ordering::SeqCst), 0);
  assert_eq!(disabled_capture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unpolled_stream_holds_endpoint_permit_until_total_deadline() {
  let (upstream_capture, upstream) = mock(MockBehavior::PendingStream).await;
  let mut settings = config(&upstream.base);
  settings.limits.max_concurrent = 4;
  settings.limits.first_byte_ms = 200;
  settings.limits.total_ms = 500;
  settings.endpoints = vec![endpoint(
    "single-capacity",
    &upstream.base,
    "endpoint-key",
    true,
    0,
    1,
  )];
  let audit = Arc::new(Audit::default());
  let server = serve(gateway(settings, audit.clone()).router()).await;
  let body = json!({"model":"public-model","messages":[{"role":"user","content":"hello"}],"max_tokens":16,"stream":true});
  let first = post(&server.base, body.clone())
    .send()
    .await
    .expect("first response");
  assert_eq!(first.status(), StatusCode::OK);
  assert_eq!(upstream_capture.calls.load(Ordering::SeqCst), 1);

  let blocked = post(&server.base, body.clone())
    .send()
    .await
    .expect("capacity response");
  assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
  let error: Value = blocked.json().await.expect("capacity error JSON");
  assert_eq!(error["error"]["code"], "endpoint_capacity");
  assert_eq!(upstream_capture.calls.load(Ordering::SeqCst), 1);

  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if audit
        .records
        .lock()
        .expect("audit records")
        .iter()
        .any(|record| {
          record.model == "public-model" && matches!(record.status, AuditStatus::Uncertain)
        })
      {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("uncertain outcome persisted at total deadline");

  // Keep `first` unpolled and alive: the endpoint lease must still be released by the deadline.
  let retry = tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let response = post(&server.base, body.clone())
        .send()
        .await
        .expect("retry response");
      if response.status() == StatusCode::OK {
        break response;
      }
      assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("endpoint released after deadline");
  assert_eq!(upstream_capture.calls.load(Ordering::SeqCst), 2);
  drop(retry);
  drop(first);
}
