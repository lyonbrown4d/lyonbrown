mod support;
use axum::{
  Router,
  body::Body,
  http::{Request, header},
  routing::post,
};
use gateway_core::AuditStatus;
use serde_json::json;
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
  },
  time::Duration,
};
use support::*;
use tower::ServiceExt;

struct DropSignal(Arc<AtomicBool>);
impl Drop for DropSignal {
  fn drop(&mut self) {
    self.0.store(true, Ordering::SeqCst);
  }
}
#[tokio::test]
async fn total_deadline_cancels_upstream_even_if_downstream_body_is_never_polled() {
  let dropped = Arc::new(AtomicBool::new(false));
  let calls = Arc::new(AtomicUsize::new(0));
  let signal = dropped.clone();
  let attempts = calls.clone();
  let upstream = serve(Router::new().route("/v1/chat/completions", post(move || {
    attempts.fetch_add(1, Ordering::SeqCst);
    let signal = signal.clone();
    async move {
      let body = Body::from_stream(async_stream::stream! {
        let _signal = DropSignal(signal);
        yield Ok::<_, std::io::Error>("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n");
        std::future::pending::<()>().await;
      });
      ([(header::CONTENT_TYPE, "text/event-stream")], body)
    }
  }))).await;
  let mut settings = config(&upstream.base);
  settings.limits.max_concurrent = 1;
  settings.limits.first_byte_ms = 100;
  settings.limits.total_ms = 150;
  let audit = Arc::new(Audit::default());
  let (gateway, _server) = gateway(settings, audit.clone()).await;
  let mut input = completion("public-model");
  input["stream"] = json!(true);
  let request = Request::builder()
    .method("POST")
    .uri("/v1/chat/completions")
    .header("authorization", format!("Bearer {TOKEN}"))
    .header("content-type", "application/json")
    .body(Body::from(input.to_string()))
    .expect("request");
  let response = gateway.router().oneshot(request).await.expect("response");
  assert_eq!(response.status(), 200);
  // Keep the body alive without polling it: timeout and cancellation must be independent.
  tokio::time::timeout(Duration::from_secs(2), async {
    while !dropped.load(Ordering::SeqCst) || gateway.observations().available_capacity != 1 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("upstream and permit released on total deadline");
  assert_eq!(calls.load(Ordering::SeqCst), 1);
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
  drop(response);
}

#[tokio::test]
async fn early_done_keeps_admission_until_unpolled_body_is_dropped() {
  let calls = Arc::new(AtomicUsize::new(0));
  let attempts = calls.clone();
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(move || {
      attempts.fetch_add(1, Ordering::SeqCst);
      async {
        (
          [(header::CONTENT_TYPE, "text/event-stream")],
          "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
        )
      }
    }),
  ))
  .await;
  let mut settings = config(&upstream.base);
  settings.limits.max_concurrent = 1;
  settings.limits.first_byte_ms = 1000;
  settings.limits.total_ms = 5000;
  let audit = Arc::new(Audit::default());
  let (gateway, _server) = gateway(settings, audit.clone()).await;
  let mut input = completion("public-model");
  input["stream"] = json!(true);
  let request = || {
    Request::builder()
      .method("POST")
      .uri("/v1/chat/completions")
      .header("authorization", format!("Bearer {TOKEN}"))
      .header("content-type", "application/json")
      .body(Body::from(input.to_string()))
      .expect("request")
  };
  let first = gateway
    .router()
    .oneshot(request())
    .await
    .expect("first response");
  assert_eq!(first.status(), 200);
  tokio::time::timeout(Duration::from_secs(2), async {
    while !audit
      .records
      .lock()
      .expect("audit")
      .iter()
      .any(|record| matches!(record.status, AuditStatus::Succeeded))
    {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("terminal persistence");
  assert_eq!(gateway.observations().available_capacity, 0);
  assert_eq!(
    gateway
      .router()
      .oneshot(request())
      .await
      .expect("capacity response")
      .status(),
    429
  );
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  drop(first);
  tokio::time::timeout(Duration::from_secs(2), async {
    while gateway.observations().available_capacity != 1 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("body drop releases capacity");
  assert_eq!(calls.load(Ordering::SeqCst), 1);
}
