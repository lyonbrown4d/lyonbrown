mod support;
use axum::http::StatusCode;
use gateway_core::{AuditRecord, AuditStatus};
use support::*;

#[tokio::test]
async fn policy_post_validates_and_applies_cas_to_persistent_and_live_state() {
  let fixture = Fixture::new("local-model").await;
  let (cookie, csrf) = fixture.login().await;
  let mut policy = fixture.engine.snapshot().policy().clone();
  policy.minimum_confidence = 0.85;
  let document = serde_json::to_string(&policy).expect("document");
  let submit = form(&[
    ("csrf", &csrf),
    ("expected_revision", "rules-v1"),
    ("revision", "policy-v2"),
    ("document", &document),
  ]);
  let response = fixture
    .send(
      "POST",
      "/ui/policy",
      Some(&cookie),
      Some(ORIGIN),
      submit.clone(),
    )
    .await;
  assert_eq!(response.status(), StatusCode::SEE_OTHER);
  assert_eq!(fixture.engine.snapshot().policy().revision, "policy-v2");
  let persisted = fixture
    .store
    .load_policy()
    .await
    .expect("load")
    .expect("policy");
  assert_eq!(persisted.revision, "policy-v2");
  assert_eq!(persisted.minimum_confidence, 0.85);
  let stale = fixture
    .send("POST", "/ui/policy", Some(&cookie), Some(ORIGIN), submit)
    .await;
  assert_eq!(stale.status(), StatusCode::CONFLICT);
  let malformed = form(&[
    ("csrf", &csrf),
    ("expected_revision", "policy-v2"),
    ("revision", "policy-v3"),
    ("document", "{}"),
  ]);
  assert_eq!(
    fixture
      .send("POST", "/ui/policy", Some(&cookie), Some(ORIGIN), malformed)
      .await
      .status(),
    StatusCode::UNPROCESSABLE_ENTITY
  );
  assert_eq!(
    fixture
      .store
      .load_policy()
      .await
      .expect("load")
      .expect("policy")
      .revision,
    "policy-v2"
  );
  let no_csrf = form(&[
    ("csrf", "bad"),
    ("expected_revision", "policy-v2"),
    ("revision", "policy-v3"),
    ("document", &document),
  ]);
  assert_eq!(
    fixture
      .send("POST", "/ui/policy", Some(&cookie), Some(ORIGIN), no_csrf)
      .await
      .status(),
    StatusCode::FORBIDDEN
  );
}
#[tokio::test]
async fn audit_pagination_and_policy_textarea_escape_persisted_content() {
  let fixture = Fixture::new("local-model").await;
  let (cookie, _) = fixture.login().await;
  for index in 0..3 {
    fixture
      .store
      .record_audit(AuditRecord {
        request_id: format!("request-{index}"),
        model: "local-model".into(),
        status: AuditStatus::Pending,
        elapsed_ms: index,
        policy_revision: "rules-v1".into(),
      })
      .await
      .expect("audit");
  }
  let response = fixture
    .send(
      "GET",
      "/ui/requests?limit=2&offset=0",
      Some(&cookie),
      None,
      String::new(),
    )
    .await;
  assert_eq!(response.status(), StatusCode::OK);
  let html = body(response).await;
  assert!(html.contains("request-2") && html.contains("request-1") && !html.contains("request-0"));
  assert!(html.contains("offset=2"));
  assert_eq!(
    fixture
      .send(
        "GET",
        "/ui/requests?limit=201",
        Some(&cookie),
        None,
        String::new()
      )
      .await
      .status(),
    StatusCode::BAD_REQUEST
  );
  let mut malicious = fixture.engine.snapshot().policy().clone();
  malicious.domains[0]
    .terms
    .push("</textarea><script>alert(1)</script>".into());
  malicious.revision = "escaped-policy".into();
  fixture.engine.replace_policy(malicious).expect("policy");
  let html = body(
    fixture
      .send("GET", "/ui/policy", Some(&cookie), None, String::new())
      .await,
  )
  .await;
  assert!(!html.contains("<script>"));
  assert!(html.contains("&lt;script&gt;"));
}
