mod support;
use axum::http::{StatusCode, header};
use gateway_web::{dto::KeysView, key_api::OneTimeToken};
use support::*;

fn draft(csrf: &str) -> String {
  form(&[
    ("csrf", csrf),
    ("label", "project production"),
    ("project_id", "project-a"),
    ("allowed_models", "local-model"),
    ("max_output_tokens", "200"),
    ("token_budget", "1000"),
    ("ttl_seconds", "3600"),
  ])
}
#[tokio::test]
async fn key_admin_requires_superadmin_origin_and_session_bound_csrf() {
  let fixture = Fixture::new("local-model").await;
  let page = fixture
    .send("GET", "/ui/keys", None, None, String::new())
    .await;
  assert_eq!(page.status(), StatusCode::SEE_OTHER);
  assert_eq!(page.headers()[header::LOCATION], "/ui/login");
  assert_eq!(
    fixture
      .send("GET", "/ui/api/keys", None, None, String::new())
      .await
      .status(),
    StatusCode::UNAUTHORIZED
  );
  let (cookie, csrf) = fixture.login().await;
  let (_, other_csrf) = fixture.login().await;
  for (cookie, origin, submitted, status) in [
    (None, Some(ORIGIN), csrf.as_str(), StatusCode::UNAUTHORIZED),
    (
      Some(cookie.as_str()),
      Some("https://attacker.example"),
      csrf.as_str(),
      StatusCode::FORBIDDEN,
    ),
    (
      Some(cookie.as_str()),
      Some(ORIGIN),
      "incorrect",
      StatusCode::FORBIDDEN,
    ),
    (
      Some(cookie.as_str()),
      Some(ORIGIN),
      other_csrf.as_str(),
      StatusCode::FORBIDDEN,
    ),
  ] {
    assert_eq!(
      fixture
        .send("POST", "/ui/keys/create", cookie, origin, draft(submitted))
        .await
        .status(),
      status
    );
    assert_eq!(
      fixture
        .send(
          "POST",
          "/ui/keys/revoke",
          cookie,
          origin,
          form(&[("csrf", submitted), ("id", "absent")])
        )
        .await
        .status(),
      status
    );
  }
  assert!(
    fixture
      .store
      .list_access_tokens()
      .await
      .expect("keys")
      .is_empty()
  );
}

#[tokio::test]
async fn issuance_returns_secret_once_metadata_never_contains_it_and_revoke_disables_access() {
  let fixture = Fixture::new("local-model").await;
  let (cookie, csrf) = fixture.login().await;
  let created = fixture
    .send(
      "POST",
      "/ui/keys/create",
      Some(&cookie),
      Some(ORIGIN),
      draft(&csrf),
    )
    .await;
  assert_eq!(created.status(), StatusCode::OK);
  assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
  let response = body(created).await;
  let secret: String = serde_json::from_str(&response).expect("one-time secret");
  let token: OneTimeToken = serde_json::from_str(&response).expect("redacted wrapper");
  assert!(!format!("{token:?}").contains(&secret));
  assert_eq!(secret.len(), 64);
  let listed = fixture
    .send("GET", "/ui/api/keys", Some(&cookie), None, String::new())
    .await;
  assert_eq!(listed.status(), StatusCode::OK);
  let metadata = body(listed).await;
  assert!(!metadata.contains(&secret));
  assert!(!metadata.contains("token_hash"));
  let view: KeysView = serde_json::from_str(&metadata).expect("metadata DTO");
  assert_eq!(view.keys.len(), 1);
  let key = &view.keys[0];
  assert_eq!(key.project_id, "project-a");
  assert_eq!(key.allowed_models, ["local-model"]);
  assert_eq!(key.max_output_tokens, Some(200));
  assert_eq!(key.token_budget, Some(1000));
  assert_eq!(key.tokens_charged, 0);
  let principal = fixture
    .store
    .authenticate_access_token(&secret, key.expires_at - 1)
    .await
    .expect("authenticate")
    .expect("valid");
  assert_eq!(principal.project_id, "project-a");
  let page = fixture
    .send("GET", "/ui/keys", Some(&cookie), None, String::new())
    .await;
  assert_eq!(page.status(), StatusCode::OK);
  let page = body(page).await;
  assert!(page.contains("project-a") && page.contains("/ui/keys/create"));
  assert!(!page.contains(&secret));
  let revoked = fixture
    .send(
      "POST",
      "/ui/keys/revoke",
      Some(&cookie),
      Some(ORIGIN),
      form(&[("csrf", &csrf), ("id", &key.id)]),
    )
    .await;
  assert_eq!(revoked.status(), StatusCode::SEE_OTHER);
  assert_eq!(revoked.headers()[header::LOCATION], "/ui/keys?revoked=true");
  assert!(
    !fixture
      .store
      .authorize_access_token(&secret, key.expires_at - 1)
      .await
      .expect("revoked")
  );
  assert!(fixture.store.list_access_tokens().await.expect("metadata")[0].revoked);
}

#[tokio::test]
async fn invalid_scope_and_budget_fields_do_not_create_keys_or_reflect_input() {
  let fixture = Fixture::new("local-model").await;
  let (cookie, csrf) = fixture.login().await;
  for (field, value) in [
    ("project_id", ""),
    ("allowed_models", "model,,other"),
    ("max_output_tokens", "-1"),
    ("token_budget", "18446744073709551615"),
    ("ttl_seconds", "0"),
    ("ttl_seconds", "31536001"),
  ] {
    let mut fields = vec![
      ("csrf", csrf.as_str()),
      ("label", "untrusted-unique-label"),
      ("project_id", "project-a"),
      ("allowed_models", "local-model"),
      ("max_output_tokens", ""),
      ("token_budget", ""),
      ("ttl_seconds", "3600"),
    ];
    fields
      .iter_mut()
      .find(|(name, _)| *name == field)
      .expect("field")
      .1 = value;
    let response = fixture
      .send(
        "POST",
        "/ui/keys/create",
        Some(&cookie),
        Some(ORIGIN),
        form(&fields),
      )
      .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
    assert!(!body(response).await.contains("untrusted-unique-label"));
  }
  assert!(
    fixture
      .store
      .list_access_tokens()
      .await
      .expect("keys")
      .is_empty()
  );
}
