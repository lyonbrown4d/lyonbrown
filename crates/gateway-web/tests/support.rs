#![allow(dead_code)]
use axum::{
  Router,
  body::Body,
  http::{Request, Response, header},
};
use decision_core::{DecisionEngine, ModelTier, Policy};
use gateway_core::{Gateway, GatewayConfig, Limits, ModelConfig, Secret};
use gateway_storage::GatewayStorage;
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

pub const USERNAME: &str = "super.admin";
pub const PASSWORD: &str = "test-admin-password-not-a-real-secret";
pub const ORIGIN: &str = "http://127.0.0.1:31337";
pub struct Fixture {
  pub router: Router,
  pub store: Arc<GatewayStorage>,
  pub engine: Arc<DecisionEngine>,
  pub gateway: Arc<Gateway>,
  _directory: tempfile::TempDir,
}
impl Fixture {
  pub async fn new(model_id: &str) -> Self {
    Self::with_password(model_id, PASSWORD).await
  }
  pub async fn with_password(model_id: &str, password: &str) -> Self {
    let directory = tempfile::tempdir().expect("directory");
    let store =
      Arc::new(GatewayStorage::open(directory.path().join("gateway.sqlite")).expect("store"));
    store
      .bootstrap_super_admin(USERNAME, password)
      .await
      .expect("admin");
    let policy = Policy::default();
    store
      .save_policy(policy.clone(), None)
      .await
      .expect("initial policy");
    let engine = Arc::new(DecisionEngine::new(policy, vec![]).expect("engine"));
    let gateway = Arc::new(
      Gateway::new(
        GatewayConfig {
          token: Secret::new("api-token-at-least-24-characters"),
          models: vec![ModelConfig {
            id: model_id.into(),
            upstream_model: "upstream-private-model".into(),
            api_base: "http://127.0.0.1:32123/v1".into(),
            api_key: Some(Secret::new("provider-private-key")),
            api_key_env: None,
            tier: ModelTier::Balanced,
            local_inference: true,
            capabilities: ["text".into()].into(),
            context_tokens: Some(4096),
          }],
          limits: Limits::default(),
          constraints: Default::default(),
        },
        engine.clone(),
        store.clone(),
      )
      .expect("gateway"),
    );
    let router = gateway_web::router(
      store.clone(),
      gateway.clone(),
      engine.clone(),
      ORIGIN.into(),
    );
    Self {
      router,
      store,
      engine,
      gateway,
      _directory: directory,
    }
  }
  pub async fn send(
    &self,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
    body: String,
  ) -> Response<Body> {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
      request = request.header(header::COOKIE, cookie);
    }
    if let Some(origin) = origin {
      request = request.header(header::ORIGIN, origin);
    }
    if method == "POST" {
      request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    self
      .router
      .clone()
      .oneshot(request.body(Body::from(body)).expect("request"))
      .await
      .expect("response")
  }
  pub async fn login(&self) -> (String, String) {
    let response = self
      .send(
        "POST",
        "/ui/login",
        None,
        Some(ORIGIN),
        form(&[("username", USERNAME), ("password", PASSWORD)]),
      )
      .await;
    assert_eq!(response.status(), 303);
    let mut csrf = String::new();
    let cookies: Vec<_> = response
      .headers()
      .get_all(header::SET_COOKIE)
      .iter()
      .map(|value| {
        let value = value.to_str().expect("cookie");
        assert!(value.contains("HttpOnly"));
        assert!(value.contains("SameSite=Strict"));
        assert!(value.contains("Path=/"));
        let pair = value.split(';').next().expect("pair");
        if let Some(value) = pair.strip_prefix("lyonbrown_csrf=") {
          csrf = value.into();
        }
        pair.to_owned()
      })
      .collect();
    assert_eq!(cookies.len(), 2);
    assert_eq!(csrf.len(), 64);
    (cookies.join("; "), csrf)
  }
}
pub fn form(values: &[(&str, &str)]) -> String {
  url::form_urlencoded::Serializer::new(String::new())
    .extend_pairs(values.iter().copied())
    .finish()
}
pub async fn body(response: Response<Body>) -> String {
  String::from_utf8(
    response
      .into_body()
      .collect()
      .await
      .expect("body")
      .to_bytes()
      .to_vec(),
  )
  .expect("UTF8")
}
