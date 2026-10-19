mod support;
use axum::{Router, body::Body, http::header, routing::post};
use gateway_core::AuditStatus;
use serde_json::json;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use support::*;

#[tokio::test]
async fn premature_sse_eof_is_uncertain_and_never_replayed() {
  let calls = Arc::new(AtomicUsize::new(0));
  let count = calls.clone();
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(move || {
      count.fetch_add(1, Ordering::SeqCst);
      async {
        (
          [(header::CONTENT_TYPE, "text/event-stream")],
          "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        )
      }
    }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(config(&upstream.base), audit.clone()).await;
  let mut body = completion("public-model");
  body["stream"] = json!(true);
  // An immediate upstream EOF can abort the connection before downstream headers flush.
  if let Ok(response) = support::post(&server.base, body).send().await {
    assert_eq!(response.status(), 200);
    assert!(response.text().await.is_err());
  }
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
}
#[tokio::test]
async fn terminal_marker_split_across_network_chunks_completes() {
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(|| async {
      let body = Body::from_stream(async_stream::stream! {
        yield Ok::<_, std::io::Error>("data: {\"choices\":[{\"index\":0,\"delta\":{}}]}\n\ndata: [DO");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        yield Ok("NE]\n\n");
      });
      ([(header::CONTENT_TYPE, "text/event-stream")], body)
    }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(config(&upstream.base), audit.clone()).await;
  let mut body = completion("public-model");
  body["stream"] = json!(true);
  let response = support::post(&server.base, body)
    .send()
    .await
    .expect("response");
  assert_eq!(
    response.text().await.expect("complete stream"),
    "data: {\"choices\":[{\"index\":0,\"delta\":{}}]}\n\ndata: [DONE]\n\n"
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
async fn invalid_success_json_is_rejected_with_uncertain_audit() {
  for payload in ["{}", "{\"error\":{\"message\":\"provider-secret\"}}"] {
    let upstream = serve(Router::new().route(
      "/v1/chat/completions",
      post(move || async move { ([(header::CONTENT_TYPE, "application/json")], payload) }),
    ))
    .await;
    let audit = Arc::new(Audit::default());
    let (_, server) = gateway(config(&upstream.base), audit.clone()).await;
    let response = support::post(&server.base, completion("public-model"))
      .send()
      .await
      .expect("response");
    assert_eq!(response.status(), 502);
    assert!(
      !response
        .text()
        .await
        .expect("redacted error")
        .contains("provider-secret")
    );
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
}
