mod support;
use axum::{
  Router,
  body::Body,
  http::{StatusCode, header},
  response::IntoResponse,
  routing::post,
};
use gateway_core::AuditStatus;
use serde_json::json;
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};
use support::*;

const FRAGMENT: &str = "data: {\"id\":\"c\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"weather\",\"arguments\":\"{\\\"city\\\":\"}}]}}]}\n\n";
const LAST: &str = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":3,\"total_tokens\":7}}\n\ndata: [DONE]\n\n";
#[tokio::test]
async fn sse_preserves_function_fragments_and_usage_exactly() {
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(|| async {
      let body = Body::from_stream(async_stream::stream! {
        yield Ok::<_, std::io::Error>(FRAGMENT);
        tokio::time::sleep(Duration::from_millis(10)).await;
        yield Ok(LAST);
      });
      ([(header::CONTENT_TYPE, "text/event-stream")], body)
    }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(config(&upstream.base), audit.clone()).await;
  let mut body = completion("public-model");
  body["stream"] = json!(true);
  body["tools"] =
    json!([{"type":"function","function":{"name":"weather","parameters":{"type":"object"}}}]);
  let response = support::post(&server.base, body)
    .send()
    .await
    .expect("stream response");
  assert_eq!(response.status(), 200);
  assert_eq!(response.headers()["content-type"], "text/event-stream");
  assert_eq!(
    response.text().await.expect("SSE text"),
    format!("{FRAGMENT}{LAST}")
  );
  assert!(matches!(
    audit
      .records
      .lock()
      .expect("audit")
      .last()
      .expect("record")
      .status,
    AuditStatus::Succeeded
  ));
}
#[tokio::test]
async fn first_byte_deadline_and_provider_error_are_redacted() {
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(|| async {
      tokio::time::sleep(Duration::from_millis(200)).await;
      ([(header::CONTENT_TYPE, "application/json")], "{}")
    }),
  ))
  .await;
  let mut settings = config(&upstream.base);
  settings.limits.first_byte_ms = 30;
  let (_, server) = gateway(settings, Arc::new(Audit::default())).await;
  assert_eq!(
    support::post(&server.base, completion("public-model"))
      .send()
      .await
      .expect("timeout")
      .status(),
    504
  );
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(|| async { (StatusCode::UNAUTHORIZED, "provider-secret-and-prompt") }),
  ))
  .await;
  let (_, server) = gateway(config(&upstream.base), Arc::new(Audit::default())).await;
  let response = support::post(&server.base, completion("public-model"))
    .send()
    .await
    .expect("provider error");
  assert_eq!(response.status(), 502);
  assert!(
    !response
      .text()
      .await
      .expect("body")
      .contains("provider-secret")
  );
}
#[tokio::test]
async fn body_first_byte_deadline_is_separate_from_headers() {
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(|| async {
      let body = Body::from_stream(async_stream::stream! {
        tokio::time::sleep(Duration::from_millis(200)).await;
        yield Ok::<_, std::io::Error>(FRAGMENT);
      });
      ([(header::CONTENT_TYPE, "text/event-stream")], body)
    }),
  ))
  .await;
  let mut settings = config(&upstream.base);
  settings.limits.first_byte_ms = 30;
  let (_, server) = gateway(settings, Arc::new(Audit::default())).await;
  let mut body = completion("public-model");
  body["stream"] = json!(true);
  assert_eq!(
    support::post(&server.base, body)
      .send()
      .await
      .expect("timeout")
      .status(),
    504
  );
}
struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
  fn drop(&mut self) {
    self.0.store(true, Ordering::SeqCst);
  }
}
async fn endless_upstream() -> (Server, Arc<AtomicBool>) {
  let dropped = Arc::new(AtomicBool::new(false));
  let signal = dropped.clone();
  let server = serve(Router::new().route(
    "/v1/chat/completions",
    post(move || {
      let signal = signal.clone();
      async move {
        let body = Body::from_stream(async_stream::stream! {
          let _guard = Dropped(signal);
          yield Ok::<_, std::io::Error>(FRAGMENT);
          loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            yield Ok(": keepalive\n\n");
          }
        });
        ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
      }
    }),
  ))
  .await;
  (server, dropped)
}
#[tokio::test]
async fn downstream_disconnect_cancels_upstream_and_releases_capacity() {
  let (upstream, dropped) = endless_upstream().await;
  let mut settings = config(&upstream.base);
  settings.limits.max_concurrent = 1;
  let audit = Arc::new(Audit::default());
  let (gateway, server) = gateway(settings, audit.clone()).await;
  let mut body = completion("public-model");
  body["stream"] = json!(true);
  let mut response = support::post(&server.base, body.clone())
    .send()
    .await
    .expect("response");
  assert!(response.chunk().await.expect("chunk").is_some());
  assert_eq!(
    support::post(&server.base, body.clone())
      .send()
      .await
      .expect("at capacity")
      .status(),
    429
  );
  drop(response);
  tokio::time::timeout(Duration::from_secs(2), async {
    while !dropped.load(Ordering::SeqCst) || gateway.observations().available_capacity == 0 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("upstream cancellation");
  let response = support::post(&server.base, body)
    .send()
    .await
    .expect("released capacity");
  assert_eq!(response.status(), 200);
  assert!(
    audit
      .records
      .lock()
      .expect("audit")
      .iter()
      .any(|record| matches!(record.status, AuditStatus::Uncertain))
  );
}
#[tokio::test]
async fn total_stream_deadline_stops_transfer_without_retry() {
  let (upstream, dropped) = endless_upstream().await;
  let mut settings = config(&upstream.base);
  settings.limits.first_byte_ms = 30;
  settings.limits.total_ms = 80;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(settings, audit.clone()).await;
  let mut body = completion("public-model");
  body["stream"] = json!(true);
  let response = support::post(&server.base, body)
    .send()
    .await
    .expect("response");
  assert_eq!(response.status(), 200);
  assert!(response.text().await.is_err());
  tokio::time::timeout(Duration::from_secs(2), async {
    while !dropped.load(Ordering::SeqCst) {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("upstream cancellation");
  assert!(matches!(
    audit
      .records
      .lock()
      .expect("audit")
      .last()
      .expect("record")
      .status,
    AuditStatus::Uncertain
  ));
}
