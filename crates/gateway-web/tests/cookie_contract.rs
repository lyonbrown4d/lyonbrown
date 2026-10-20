mod support;
use axum::{
  body::Body,
  http::{Request, StatusCode, header},
};
use cookie::{Cookie, SameSite};
use support::*;
use tower::ServiceExt;

#[tokio::test]
async fn cookie_parser_keeps_duplicate_and_malformed_credentials_rejected() {
  let fixture = Fixture::new("local-model").await;
  let (credentials, _) = fixture.login().await;
  let session = credentials.split(';').next().expect("session");
  for header_value in [
    format!("{credentials}; {session}"),
    format!("{credentials}; lyonbrown_csrf={}", "a".repeat(64)),
    credentials.replace("lyonbrown_admin=", "lyonbrown_admin=%61"),
  ] {
    let response = fixture
      .send(
        "GET",
        "/ui/models",
        Some(&header_value),
        None,
        String::new(),
      )
      .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/ui/login");
  }
  let request = Request::builder()
    .uri("/ui/models")
    .header(header::COOKIE, &credentials)
    .header(header::COOKIE, session)
    .body(Body::empty())
    .expect("request");
  let response = fixture
    .router
    .clone()
    .oneshot(request)
    .await
    .expect("response");
  assert_eq!(response.status(), StatusCode::SEE_OTHER);
  assert_eq!(response.headers()[header::LOCATION], "/ui/login");
  let response = fixture
    .send(
      "GET",
      "/ui/models",
      Some(&format!("theme=light; {credentials}")),
      None,
      String::new(),
    )
    .await;
  assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn secure_origin_issues_two_strict_http_only_cookies_with_bounded_lifetime() {
  let fixture = Fixture::new("local-model").await;
  let origin = "https://gateway.example";
  let router = gateway_web::router(
    fixture.store.clone(),
    fixture.gateway.clone(),
    fixture.engine.clone(),
    origin.into(),
  );
  let request = Request::builder()
    .method("POST")
    .uri("/ui/login")
    .header(header::ORIGIN, origin)
    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
    .body(Body::from(form(&[
      ("username", USERNAME),
      ("password", PASSWORD),
    ])))
    .expect("request");
  let response = router.oneshot(request).await.expect("response");
  assert_eq!(response.status(), StatusCode::SEE_OTHER);
  let cookies = response
    .headers()
    .get_all(header::SET_COOKIE)
    .iter()
    .map(|value| Cookie::parse(value.to_str().expect("header")).expect("cookie"))
    .collect::<Vec<_>>();
  assert_eq!(cookies.len(), 2);
  for cookie in cookies {
    assert!(matches!(
      cookie.name(),
      "lyonbrown_admin" | "lyonbrown_csrf"
    ));
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    assert_eq!(cookie.max_age().expect("lifetime").whole_seconds(), 3600);
    assert_eq!(cookie.value().len(), 64);
    assert!(!cookie.value().contains(PASSWORD));
  }
}
