mod support;
use axum::http::{StatusCode, header};
use support::*;

#[tokio::test]
async fn protected_pages_require_session_and_render_escaped_model_ids_without_secrets() {
  let fixture = Fixture::new("<script>alert('x')</script>").await;
  for path in ["/ui/models", "/ui/policy", "/ui/requests"] {
    let response = fixture.send("GET", path, None, None, String::new()).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/ui/login");
  }
  let (cookie, _) = fixture.login().await;
  let response = fixture
    .send("GET", "/ui/models", Some(&cookie), None, String::new())
    .await;
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
  assert_eq!(response.headers()[header::REFERRER_POLICY], "same-origin");
  assert!(
    response.headers()[header::CONTENT_SECURITY_POLICY]
      .to_str()
      .expect("CSP")
      .contains("frame-ancestors 'none'")
  );
  let html = body(response).await;
  assert!(html.contains("&lt;script&gt;"));
  for secret in [
    "<script>",
    PASSWORD,
    "provider-private-key",
    "upstream-private-model",
    "32123",
    "localStorage",
  ] {
    assert!(!html.contains(secret));
  }
  assert!(html.contains("/ui/assets/app.css"));
}
#[tokio::test]
async fn origin_csrf_and_logout_enforce_session_bound_mutations() {
  let fixture = Fixture::new("local-model").await;
  let (cookie, csrf) = fixture.login().await;
  assert_eq!(
    fixture
      .send(
        "POST",
        "/ui/logout",
        Some(&cookie),
        Some("https://attacker.example"),
        form(&[("csrf", &csrf)])
      )
      .await
      .status(),
    StatusCode::FORBIDDEN
  );
  assert_eq!(
    fixture
      .send(
        "POST",
        "/ui/logout",
        Some(&cookie),
        Some(ORIGIN),
        form(&[("csrf", "incorrect")])
      )
      .await
      .status(),
    StatusCode::FORBIDDEN
  );
  assert_eq!(
    fixture
      .send("GET", "/ui/logout", Some(&cookie), None, String::new())
      .await
      .status(),
    StatusCode::METHOD_NOT_ALLOWED
  );
  let (_, other_csrf) = fixture.login().await;
  assert_eq!(
    fixture
      .send(
        "POST",
        "/ui/logout",
        Some(&cookie),
        Some(ORIGIN),
        form(&[("csrf", &other_csrf)])
      )
      .await
      .status(),
    StatusCode::FORBIDDEN
  );
  let response = fixture
    .send(
      "POST",
      "/ui/logout",
      Some(&cookie),
      Some(ORIGIN),
      form(&[("csrf", &csrf)]),
    )
    .await;
  assert_eq!(response.status(), StatusCode::SEE_OTHER);
  assert!(
    response
      .headers()
      .get_all(header::SET_COOKIE)
      .iter()
      .all(|v| v.to_str().expect("cookie").contains("Max-Age=0"))
  );
  assert_eq!(
    fixture
      .send("GET", "/ui/models", Some(&cookie), None, String::new())
      .await
      .status(),
    StatusCode::SEE_OTHER
  );
}
#[tokio::test]
async fn login_requires_configured_origin_and_has_a_bounded_attempt_window() {
  let fixture = Fixture::new("local-model").await;
  let page = fixture
    .send("GET", "/ui/login", None, None, String::new())
    .await;
  assert_eq!(page.headers()[header::REFERRER_POLICY], "same-origin");
  assert_eq!(
    fixture
      .send(
        "POST",
        "/ui/login",
        None,
        None,
        form(&[("username", USERNAME), ("password", PASSWORD)])
      )
      .await
      .status(),
    StatusCode::FORBIDDEN
  );
  assert_eq!(
    fixture
      .send(
        "POST",
        "/ui/login",
        None,
        Some("null"),
        form(&[("username", USERNAME), ("password", PASSWORD)])
      )
      .await
      .status(),
    StatusCode::FORBIDDEN
  );
  for _ in 0..10 {
    assert_eq!(
      fixture
        .send(
          "POST",
          "/ui/login",
          None,
          Some(ORIGIN),
          form(&[("username", USERNAME), ("password", "incorrect")])
        )
        .await
        .status(),
      StatusCode::UNAUTHORIZED
    );
  }
  let response = fixture
    .send(
      "POST",
      "/ui/login",
      None,
      Some(ORIGIN),
      form(&[("username", USERNAME), ("password", PASSWORD)]),
    )
    .await;
  assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
  assert_eq!(response.headers()[header::RETRY_AFTER], "60");
}
#[tokio::test]
async fn expired_session_cannot_read_protected_pages() {
  let fixture = Fixture::new("local-model").await;
  let expired = fixture
    .store
    .login(USERNAME, PASSWORD, 1, 1)
    .await
    .expect("expired session");
  let cookie = format!(
    "lyonbrown_admin={}; lyonbrown_csrf={}",
    expired.token(),
    expired.csrf_token()
  );
  assert_eq!(
    fixture
      .send("GET", "/ui/models", Some(&cookie), None, String::new())
      .await
      .status(),
    StatusCode::SEE_OTHER
  );
}

#[tokio::test]
async fn username_password_failures_are_uniform_and_never_echo_passwords() {
  let fixture = Fixture::new("local-model").await;
  let html = body(
    fixture
      .send("GET", "/ui/login", None, None, String::new())
      .await,
  )
  .await;
  assert!(html.contains("autocomplete=\"username\""));
  assert!(html.contains("autocomplete=\"current-password\""));
  assert!(html.contains("name=\"username\"") && html.contains("name=\"password\""));
  assert!(!html.contains("name=\"token\""));
  let oversized = "密".repeat(50);
  for (username, password) in [
    ("unknown.admin", PASSWORD),
    (USERNAME, "incorrect-password-long-enough"),
    ("", PASSWORD),
    ("invalid name", PASSWORD),
    ("管理员", PASSWORD),
    (USERNAME, "short"),
    (USERNAME, oversized.as_str()),
  ] {
    let response = fixture
      .send(
        "POST",
        "/ui/login",
        None,
        Some(ORIGIN),
        form(&[("username", username), ("password", password)]),
      )
      .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let html = body(response).await;
    assert!(html.contains("用户名或密码无效"));
    assert!(!html.contains(password));
  }
  let legacy = fixture
    .send(
      "POST",
      "/ui/login",
      None,
      Some(ORIGIN),
      form(&[("token", "api-token-at-least-24-characters")]),
    )
    .await;
  assert_eq!(legacy.status(), StatusCode::UNAUTHORIZED);
  assert!(body(legacy).await.contains("用户名或密码无效"));
  fixture.login().await;
}

#[tokio::test]
async fn password_length_is_validated_in_bytes_and_super_admin_identity_is_required() {
  // Five Chinese characters are 15 UTF-8 bytes and satisfy the configured password contract.
  let password = "长密码测试";
  assert_eq!(password.len(), 15);
  let fixture = Fixture::with_password("local-model", password).await;
  let response = fixture
    .send(
      "POST",
      "/ui/login",
      None,
      Some(ORIGIN),
      form(&[("username", USERNAME), ("password", password)]),
    )
    .await;
  assert_eq!(response.status(), StatusCode::SEE_OTHER);
  let cookies = response
    .headers()
    .get_all(header::SET_COOKIE)
    .iter()
    .map(|value| {
      value
        .to_str()
        .expect("cookie")
        .split(';')
        .next()
        .expect("pair")
    })
    .collect::<Vec<_>>()
    .join("; ");
  let response = fixture
    .send("GET", "/ui/models", Some(&cookies), None, String::new())
    .await;
  assert_eq!(response.status(), StatusCode::OK);
  assert!(!body(response).await.contains(password));
}
