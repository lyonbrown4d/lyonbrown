mod support;
use axum::{
  body::Body,
  http::{Request, StatusCode, header},
};
use gateway_web::{
  LeptosOptions,
  api::{Login, PasswordInput},
};
use support::*;
use tower::ServiceExt;

#[test]
fn generated_server_action_input_debug_redacts_password() {
  let input = Login {
    username: USERNAME.into(),
    password: PasswordInput::from(PASSWORD.to_owned()),
  };
  assert!(!format!("{input:?}").contains(PASSWORD));
  assert!(format!("{input:?}").contains("REDACTED"));
}

#[tokio::test]
async fn server_function_resources_require_super_admin_and_return_only_safe_dtos() {
  let fixture = Fixture::new("private-model-id").await;
  for path in [
    "/ui/api/models",
    "/ui/api/policy",
    "/ui/api/requests?offset=0&limit=25",
  ] {
    let response = fixture.send("GET", path, None, None, String::new()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let output = body(response).await;
    for private in [
      "private-model-id",
      "rules-v1",
      "provider-private-key",
      PASSWORD,
    ] {
      assert!(!output.contains(private));
    }
  }
  let bearer_only = fixture
    .router
    .clone()
    .oneshot(
      Request::builder()
        .uri("/ui/api/models")
        .header(
          header::AUTHORIZATION,
          "Bearer api-token-at-least-24-characters",
        )
        .body(Body::empty())
        .expect("request"),
    )
    .await
    .expect("response");
  assert_eq!(bearer_only.status(), StatusCode::UNAUTHORIZED);
  let (cookie, csrf) = fixture.login().await;
  let response = fixture
    .send("GET", "/ui/api/models", Some(&cookie), None, String::new())
    .await;
  assert_eq!(response.status(), StatusCode::OK);
  let dto: serde_json::Value = serde_json::from_str(&body(response).await).expect("typed JSON");
  assert_eq!(dto["csrf"], csrf);
  assert_eq!(dto["models"][0]["id"], "private-model-id");
  for private in [
    "api_base",
    "api_key",
    "provider-private-key",
    "upstream-private-model",
    "32123",
    PASSWORD,
  ] {
    assert!(!dto.to_string().contains(private));
  }
  let malformed = fixture
    .send(
      "GET",
      "/ui/api/requests?offset=password-should-not-echo&limit=25",
      Some(&cookie),
      None,
      String::new(),
    )
    .await;
  assert!(!malformed.status().is_success());
  assert!(!body(malformed).await.contains("password-should-not-echo"));
}

#[tokio::test]
async fn ssr_stream_bootstrap_and_resource_scripts_share_the_csp_nonce() {
  let fixture = Fixture::new("local-model").await;
  let (cookie, _) = fixture.login().await;
  let response = fixture
    .send("GET", "/ui/models", Some(&cookie), None, String::new())
    .await;
  let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
    .to_str()
    .expect("CSP")
    .to_owned();
  assert!(!csp.contains("unsafe-inline"));
  assert!(csp.contains("'wasm-unsafe-eval'") && csp.contains("connect-src 'self'"));
  let nonce = csp
    .split("'nonce-")
    .nth(1)
    .and_then(|v| v.split('\'').next())
    .expect("nonce");
  let html = body(response).await;
  let mut scripts = 0;
  for (offset, _) in html.match_indices("<script") {
    let tag = html[offset..].split('>').next().expect("tag");
    assert!(
      tag.contains(&format!("nonce=\"{nonce}\"")),
      "script lacks matching nonce: {tag}"
    );
    scripts += 1;
  }
  assert!(scripts > 0);
  assert!(html.contains("/ui/pkg/gateway_web.js"));
  assert!(html.contains("/ui/assets/app.css"));
  let api_link = html
    .split("<a")
    .find(|tag| {
      tag
        .split('>')
        .next()
        .is_some_and(|tag| tag.contains("href=\"/swagger-ui/\""))
    })
    .expect("Swagger navigation link");
  assert!(
    api_link
      .split('>')
      .next()
      .expect("link tag")
      .contains("rel=\"external\"")
  );
}

#[tokio::test]
async fn configured_static_asset_root_serves_the_hydration_package() {
  let fixture = Fixture::new("local-model").await;
  let directory = tempfile::tempdir().expect("directory");
  let package = directory.path().join("ui/pkg");
  std::fs::create_dir_all(&package).expect("package directory");
  std::fs::write(
    package.join("gateway_web.js"),
    "export const fixture = true;",
  )
  .expect("asset");
  let options = LeptosOptions::builder()
    .output_name("gateway_web")
    .site_root(directory.path().to_str().expect("path"))
    .site_pkg_dir("ui/pkg")
    .build();
  let router = gateway_web::router_with_options(
    fixture.store.clone(),
    fixture.gateway.clone(),
    fixture.engine.clone(),
    ORIGIN.into(),
    options,
  );
  let response = router
    .oneshot(
      Request::builder()
        .uri("/ui/pkg/gateway_web.js")
        .body(Body::empty())
        .expect("request"),
    )
    .await
    .expect("response");
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(body(response).await, "export const fixture = true;");
}

#[tokio::test]
async fn raw_server_function_bodies_are_limited_before_decoding_credentials() {
  let fixture = Fixture::new("local-model").await;
  let secret = "oversized-password-must-not-echo";
  let payload = format!("username={USERNAME}&password={}", secret.repeat(80_000));
  let response = fixture
    .router
    .clone()
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/ui/login")
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::CONTENT_LENGTH, payload.len())
        .body(Body::from(payload))
        .expect("request"),
    )
    .await
    .expect("response");
  assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
  assert!(!body(response).await.contains(secret));
}
