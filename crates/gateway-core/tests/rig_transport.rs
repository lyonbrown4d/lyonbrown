mod support;
use axum::{
  Router,
  body::{Body, Bytes},
  extract::Json,
  http::{StatusCode, header},
  routing::post,
};
use gateway_core::AuditStatus;
use serde_json::{Value, json};
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
  },
  time::Duration,
};
use support::*;

const TWO_CHOICES: &str = r#"{"id":"multi","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_a","type":"function","function":{"name":"weather","arguments":"{\"city\":\"杭州\"}"}}]},"finish_reason":"tool_calls"},{"index":1,"message":{"role":"assistant","content":"另一候选","refusal":null},"finish_reason":"length"}],"usage":{"prompt_tokens":4,"completion_tokens":6,"total_tokens":10,"completion_tokens_details":{"reasoning_tokens":2}},"provider_extension":{"kept":true}}"#;

#[tokio::test]
async fn multiple_choices_tools_finish_reasons_and_usage_remain_original_json() {
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(|Json(body): Json<Value>| async move {
      assert_eq!(body["n"], 2);
      assert_eq!(body["model"], "actual-upstream-model");
      ([(header::CONTENT_TYPE, "application/json")], TWO_CHOICES)
    }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(config(&upstream.base), audit.clone()).await;
  let mut body = completion("public-model");
  body["n"] = json!(2);
  let response = support::post(&server.base, body)
    .send()
    .await
    .expect("response");
  assert_eq!(response.status(), 200);
  assert_eq!(response.text().await.expect("JSON"), TWO_CHOICES);
  assert!(matches!(
    audit
      .records
      .lock()
      .expect("audit")
      .last()
      .expect("terminal")
      .status,
    AuditStatus::Succeeded
  ));
}

#[tokio::test]
async fn split_utf8_and_interleaved_candidates_preserve_sse_bytes() {
  const SSE: &str = "data: {\"choices\":[{\"index\":1,\"delta\":{\"content\":\"杭州\"},\"finish_reason\":null},{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":3,\"total_tokens\":7}}\n\ndata: [DONE]\n\n";
  let split = SSE.find('杭').expect("Chinese fixture") + 1;
  let (release, next) = tokio::sync::oneshot::channel();
  let next = Arc::new(std::sync::Mutex::new(Some(next)));
  let upstream = serve(Router::new().route(
    "/v1/chat/completions",
    post(move || {
      let next = next.lock().expect("gate").take().expect("one request");
      async move {
        let body = Body::from_stream(async_stream::stream! {
          yield Ok::<_, std::io::Error>(Bytes::from_static(&SSE.as_bytes()[..split]));
          next.await.expect("first fragment forwarded");
          yield Ok(Bytes::from_static(&SSE.as_bytes()[split..]));
        });
        ([(header::CONTENT_TYPE, "text/event-stream")], body)
      }
    }),
  ))
  .await;
  let audit = Arc::new(Audit::default());
  let (_, server) = gateway(config(&upstream.base), audit.clone()).await;
  let mut body = completion("public-model");
  body["n"] = json!(2);
  body["stream"] = json!(true);
  let mut response = support::post(&server.base, body)
    .send()
    .await
    .expect("response");
  assert_eq!(response.status(), 200);
  let mut received = Vec::new();
  while received.len() < split {
    received.extend_from_slice(
      &tokio::time::timeout(Duration::from_secs(2), response.chunk())
        .await
        .expect("incomplete codepoint remains streamed")
        .expect("fragment")
        .expect("prefix"),
    );
  }
  assert_eq!(received, SSE.as_bytes()[..split]);
  release.send(()).expect("release second fragment");
  received.extend_from_slice(&response.bytes().await.expect("SSE"));
  assert_eq!(received, SSE.as_bytes());
  assert!(matches!(
    audit
      .records
      .lock()
      .expect("audit")
      .last()
      .expect("terminal")
      .status,
    AuditStatus::Succeeded
  ));
}

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
  fn drop(&mut self) {
    self.0.store(true, Ordering::SeqCst);
  }
}

#[tokio::test]
async fn rejected_huge_or_stalled_bodies_are_dropped_before_rig_buffers_them() {
  for (status, huge, streaming, expected, failed) in [
    (StatusCode::TOO_MANY_REQUESTS, true, false, 429, true),
    (StatusCode::SERVICE_UNAVAILABLE, false, true, 502, false),
  ] {
    let dropped = Arc::new(AtomicBool::new(false));
    let signal = dropped.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let upstream = serve(Router::new().route(
      "/v1/chat/completions",
      post(move || {
        count.fetch_add(1, Ordering::SeqCst);
        let guard = Dropped(signal.clone());
        async move {
          let body = Body::from_stream(async_stream::stream! {
            let _guard = guard;
            if huge {
              yield Ok::<_, std::io::Error>(Bytes::from(vec![b'x'; 1024 * 1024]));
            }
            std::future::pending::<()>().await;
          });
          (
            status,
            [
              (header::CONTENT_TYPE, "application/json"),
              (header::SET_COOKIE, "provider-secret=hidden"),
            ],
            body,
          )
        }
      }),
    ))
    .await;
    let mut settings = config(&upstream.base);
    settings.limits.first_byte_ms = 5_000;
    settings.limits.total_ms = 5_000;
    settings.limits.max_response_bytes = 1024;
    let audit = Arc::new(Audit::default());
    let (_, server) = gateway(settings, audit.clone()).await;
    let mut body = completion("public-model");
    body["stream"] = json!(streaming);
    let response = tokio::time::timeout(
      Duration::from_secs(2),
      support::post(&server.base, body).send(),
    )
    .await
    .expect("reject without waiting for error body")
    .expect("response");
    assert_eq!(response.status(), expected);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let text = response.text().await.expect("safe error");
    assert!(!text.contains("provider-secret"));
    assert!(text.len() < 1024);
    tokio::time::timeout(Duration::from_secs(2), async {
      while !dropped.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("upstream body released");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let records = audit.records.lock().expect("audit");
    assert!(matches!(
      records.first().expect("intent").status,
      AuditStatus::Pending
    ));
    let terminal = records.last().expect("terminal").status;
    assert!(if failed {
      matches!(terminal, AuditStatus::Failed)
    } else {
      matches!(terminal, AuditStatus::Uncertain)
    });
  }
}

#[tokio::test]
async fn malformed_encoding_incomplete_termination_and_trailing_frames_remain_uncertain() {
  for payload in [
    b"data: {\"choices\":[{\"delta\":{}}]}\n\ndata: [DONE]\n\ndata: {\"choices\":[".as_slice(),
    b"data: {\"choices\":[{\"delta\":{\"content\":\"\xff\"}}]}\n\ndata: [DONE]\n\n".as_slice(),
    b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n".as_slice(),
  ] {
    let upstream = serve(Router::new().route(
      "/v1/chat/completions",
      post(move || async move {
        (
          [(header::CONTENT_TYPE, "text/event-stream")],
          Bytes::from_static(payload),
        )
      }),
    ))
    .await;
    let audit = Arc::new(Audit::default());
    let (_, server) = gateway(config(&upstream.base), audit.clone()).await;
    let mut body = completion("public-model");
    body["stream"] = json!(true);
    if let Ok(response) = support::post(&server.base, body).send().await {
      if response.status() == 200 {
        assert!(response.bytes().await.is_err());
      } else {
        assert_eq!(response.status(), 502);
      }
    }
    assert!(matches!(
      audit
        .records
        .lock()
        .expect("audit")
        .last()
        .expect("terminal")
        .status,
      AuditStatus::Uncertain
    ));
  }
}
