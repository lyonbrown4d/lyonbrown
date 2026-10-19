mod support;
use axum::{
  body::Body,
  http::{Request, StatusCode, header},
};
use support::*;
use tower::ServiceExt;

#[tokio::test]
async fn native_forms_redirect_errors_to_safe_ssr_notices_without_echoing_inputs() {
  let fixture = Fixture::new("local-model").await;
  let submitted = "wrong-password-native-never-echo";
  let request = Request::builder()
    .method("POST")
    .uri("/ui/login")
    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
    .header(header::ACCEPT, "text/html")
    .header(header::ORIGIN, ORIGIN)
    .header(
      header::REFERER,
      "https://untrusted.example/?password=must-not-be-copied",
    )
    .body(Body::from(form(&[
      ("username", USERNAME),
      ("password", submitted),
    ])))
    .expect("request");
  let response = fixture
    .router
    .clone()
    .oneshot(request)
    .await
    .expect("response");
  assert_eq!(response.status(), StatusCode::SEE_OTHER);
  assert_eq!(
    response.headers()[header::LOCATION],
    "/ui/login?error=credentials"
  );
  assert!(!body(response).await.contains(submitted));
  let page = body(
    fixture
      .send(
        "GET",
        "/ui/login?error=credentials",
        None,
        None,
        String::new(),
      )
      .await,
  )
  .await;
  assert!(page.contains("用户名或密码无效"));
  assert!(!page.contains(submitted));

  let (cookie, csrf) = fixture.login().await;
  let original = fixture
    .store
    .load_policy()
    .await
    .expect("policy")
    .expect("persisted policy");
  let document = "{malformed-policy-native-never-echo";
  let request = Request::builder()
    .method("POST")
    .uri("/ui/policy")
    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
    .header(header::ACCEPT, "text/html")
    .header(header::ORIGIN, ORIGIN)
    .header(header::COOKIE, &cookie)
    .header(header::REFERER, format!("{ORIGIN}/ui/policy"))
    .body(Body::from(form(&[
      ("csrf", &csrf),
      ("expected_revision", "rules-v1"),
      ("revision", "must-not-persist"),
      ("document", document),
    ])))
    .expect("request");
  let response = fixture
    .router
    .clone()
    .oneshot(request)
    .await
    .expect("response");
  assert_eq!(response.status(), StatusCode::SEE_OTHER);
  assert_eq!(
    response.headers()[header::LOCATION],
    "/ui/policy?error=validation"
  );
  assert!(!body(response).await.contains(document));
  let page = body(
    fixture
      .send(
        "GET",
        "/ui/policy?error=validation",
        Some(&cookie),
        None,
        String::new(),
      )
      .await,
  )
  .await;
  assert!(page.contains("策略未保存。请检查版本名称和策略内容。"));
  assert!(!page.contains(document));
  assert_eq!(
    fixture.store.load_policy().await.expect("policy"),
    Some(original.clone())
  );
  assert_eq!(fixture.engine.snapshot().policy(), &original);
}
