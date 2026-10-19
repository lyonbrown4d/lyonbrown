mod support;
use async_trait::async_trait;
use axum::{Router, body::Body, http::header, routing::post};
use decision_core::{DecisionEngine, Policy};
use gateway_core::{AuditError, AuditRecord, AuditSink, AuditStatus, Gateway};
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};
use support::*;
use tokio::sync::Notify;

#[derive(Default)]
struct SlowTerminal {
  started: Notify,
  release: Notify,
  committed: AtomicBool,
}
#[async_trait]
impl AuditSink for SlowTerminal {
  async fn record(&self, record: AuditRecord) -> Result<(), AuditError> {
    if !matches!(record.status, AuditStatus::Pending) {
      self.started.notify_one();
      self.release.notified().await;
      self.committed.store(true, Ordering::SeqCst);
    }
    Ok(())
  }
}
struct DropSignal(Arc<AtomicBool>);
impl Drop for DropSignal {
  fn drop(&mut self) {
    self.0.store(true, Ordering::SeqCst);
  }
}
#[tokio::test]
async fn first_byte_timeout_drops_network_before_waiting_for_terminal_persistence() {
  let dropped = Arc::new(AtomicBool::new(false));
  let signal = dropped.clone();
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(move || {
      let signal = signal.clone();
      async {
        let body = Body::from_stream(async_stream::stream! {
          let _signal = DropSignal(signal);
          tokio::time::sleep(Duration::from_secs(5)).await;
          yield Ok::<_, std::io::Error>("data: {\"choices\":[]}\n\n");
        });
        ([(header::CONTENT_TYPE, "text/event-stream")], body)
      }
    }),
  ))
  .await;
  let mut settings = config(&upstream.base);
  settings.limits.first_byte_ms = 100;
  settings.limits.total_ms = 5000;
  let audit = Arc::new(SlowTerminal::default());
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("engine"));
  let gateway = Gateway::new(settings, engine, audit.clone()).expect("gateway");
  let server = serve(gateway.router()).await;
  let mut input = completion("public-model");
  input["stream"] = serde_json::json!(true);
  let request = support::post(&server.base, input);
  let response = tokio::spawn(async move { request.send().await.expect("response") });
  tokio::time::timeout(Duration::from_secs(2), audit.started.notified())
    .await
    .expect("terminal write entered");
  tokio::time::timeout(Duration::from_secs(2), async {
    while !dropped.load(Ordering::SeqCst) {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("network cancellation must not wait for the audit sink");
  audit.release.notify_one();
  assert_eq!(response.await.expect("response task").status(), 504);
}

#[tokio::test]
async fn cancelled_handler_retains_terminal_write_responsibility_past_one_second() {
  use tower::ServiceExt;
  let entered = Arc::new(Notify::new());
  let signal = entered.clone();
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(move || {
      let signal = signal.clone();
      async move {
        signal.notify_one();
        tokio::time::sleep(Duration::from_secs(5)).await;
        ([(header::CONTENT_TYPE, "application/json")], "{}")
      }
    }),
  ))
  .await;
  let mut settings = config(&upstream.base);
  settings.limits.first_byte_ms = 1000;
  settings.limits.total_ms = 5000;
  settings.limits.max_concurrent = 1;
  let audit = Arc::new(SlowTerminal::default());
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("engine"));
  let gateway = Gateway::new(settings, engine, audit.clone()).expect("gateway");
  let request = axum::http::Request::builder()
    .method("POST")
    .uri("/v1/chat/completions")
    .header("authorization", format!("Bearer {TOKEN}"))
    .header("content-type", "application/json")
    .body(Body::from(completion("public-model").to_string()))
    .expect("request");
  let response = tokio::spawn(gateway.router().oneshot(request));
  tokio::time::timeout(Duration::from_secs(2), entered.notified())
    .await
    .expect("upstream entered");
  response.abort();
  assert!(
    response
      .await
      .expect_err("handler cancelled")
      .is_cancelled()
  );
  tokio::time::timeout(Duration::from_secs(2), audit.started.notified())
    .await
    .expect("terminal cleanup entered");
  tokio::time::sleep(Duration::from_millis(1200)).await;
  assert_eq!(gateway.observations().available_capacity, 0);
  assert!(!audit.committed.load(Ordering::SeqCst));
  audit.release.notify_one();
  tokio::time::timeout(Duration::from_secs(2), async {
    while !audit.committed.load(Ordering::SeqCst) || gateway.observations().available_capacity != 1
    {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("cancelled handler's terminal write completed");
}
