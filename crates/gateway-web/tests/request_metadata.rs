mod support;
use axum::http::StatusCode;
use gateway_core::{AuditRecord, AuditStatus, TokenUsage};
use gateway_web::dto::RequestsView;
use std::{
  collections::BTreeSet,
  time::{SystemTime, UNIX_EPOCH},
};
use support::*;

#[tokio::test]
async fn request_metadata_exposes_safe_identity_and_usage_preserving_unknown_and_actual_zero() {
  let fixture = Fixture::new("local-model").await;
  let (cookie, _) = fixture.login().await;
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("clock")
    .as_secs();
  let key = fixture
    .store
    .create_scoped_access_token(
      "key-label-not-in-request-log",
      now,
      3600,
      "project-visible",
      BTreeSet::from([
        "local-model".into(),
        "scope-marker-not-in-request-log".into(),
      ]),
      Some(200),
      None,
    )
    .await
    .expect("key");
  let principal = fixture
    .store
    .authenticate_access_token(key.token(), now)
    .await
    .expect("authenticate")
    .expect("principal");
  let mut record = AuditRecord {
    request_id: "with-usage".into(),
    model: "local-model".into(),
    principal: Some(principal.clone()),
    reserved_tokens: 100,
    usage: None,
    status: AuditStatus::Pending,
    elapsed_ms: 10,
    policy_revision: "rules-v1".into(),
  };
  fixture
    .store
    .record_audit(record.clone())
    .await
    .expect("intent");
  record.status = AuditStatus::Succeeded;
  record.usage = Some(TokenUsage {
    input_tokens: 20,
    output_tokens: 30,
    cached_input_tokens: 10,
    reasoning_tokens: 15,
  });
  fixture
    .store
    .record_audit(record.clone())
    .await
    .expect("usage");
  record.request_id = "actual-zero".into();
  record.status = AuditStatus::Pending;
  record.usage = None;
  fixture
    .store
    .record_audit(record.clone())
    .await
    .expect("intent");
  record.status = AuditStatus::Succeeded;
  record.usage = Some(TokenUsage::default());
  fixture
    .store
    .record_audit(record.clone())
    .await
    .expect("zero usage");
  record.request_id = "unknown-usage".into();
  record.status = AuditStatus::Pending;
  record.usage = None;
  fixture
    .store
    .record_audit(record.clone())
    .await
    .expect("intent");
  record.status = AuditStatus::Uncertain;
  fixture
    .store
    .record_audit(record.clone())
    .await
    .expect("uncertain");
  record.request_id = "legacy-unknown".into();
  record.status = AuditStatus::Pending;
  record.principal = None;
  record.reserved_tokens = 0;
  fixture
    .store
    .record_audit(record)
    .await
    .expect("legacy intent");

  let response = fixture
    .send(
      "GET",
      "/ui/api/requests?offset=0&limit=25",
      Some(&cookie),
      None,
      String::new(),
    )
    .await;
  assert_eq!(response.status(), StatusCode::OK);
  let json = body(response).await;
  let view: RequestsView = serde_json::from_str(&json).expect("DTO");
  assert_eq!(view.total, 4);
  let usage = view
    .records
    .iter()
    .find(|record| record.request_id == "with-usage")
    .expect("usage row");
  assert_eq!(usage.project_id.as_deref(), Some("project-visible"));
  assert_eq!(usage.key_id.as_deref(), Some(key.metadata.id.as_str()));
  assert_eq!(usage.reserved_tokens, 100);
  assert_eq!(
    (
      usage.input_tokens,
      usage.output_tokens,
      usage.cached_input_tokens,
      usage.reasoning_tokens
    ),
    (Some(20), Some(30), Some(10), Some(15))
  );
  let zero = view
    .records
    .iter()
    .find(|record| record.request_id == "actual-zero")
    .expect("zero row");
  assert_eq!(
    (
      zero.input_tokens,
      zero.output_tokens,
      zero.cached_input_tokens,
      zero.reasoning_tokens
    ),
    (Some(0), Some(0), Some(0), Some(0))
  );
  let unknown = view
    .records
    .iter()
    .find(|record| record.request_id == "unknown-usage")
    .expect("unknown row");
  assert_eq!(unknown.project_id.as_deref(), Some("project-visible"));
  assert_eq!(
    (
      unknown.input_tokens,
      unknown.output_tokens,
      unknown.cached_input_tokens,
      unknown.reasoning_tokens
    ),
    (None, None, None, None)
  );
  let legacy = view
    .records
    .iter()
    .find(|record| record.request_id == "legacy-unknown")
    .expect("legacy row");
  assert_eq!(
    (legacy.project_id.as_ref(), legacy.key_id.as_ref()),
    (None, None)
  );

  let page = fixture
    .send("GET", "/ui/requests", Some(&cookie), None, String::new())
    .await;
  assert_eq!(page.status(), StatusCode::OK);
  let html = body(page).await;
  for text in [
    "项目 project-visible",
    "项目 未知",
    "Key 未知",
    "输入 20 / 输出 30",
    "缓存 10 / 推理 15",
    "输入 0 / 输出 0",
    "缓存 0 / 推理 0",
    "输入 未知 / 输出 未知",
    "预留 100",
  ] {
    assert!(html.contains(text), "missing {text}");
  }
  assert!(html.contains(&key.metadata.id));
  for forbidden in [
    key.token(),
    "scope-marker-not-in-request-log",
    "key-label-not-in-request-log",
    "allowed_models",
    "max_output_tokens",
    "token_hash",
    "provider-private-key",
  ] {
    assert!(!json.contains(forbidden), "leaked request DTO field");
    assert!(!html.contains(forbidden), "leaked SSR field");
  }
}
