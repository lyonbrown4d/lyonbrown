#![allow(dead_code)]
use async_trait::async_trait;
use axum::{Json, Router, extract::State, routing::post as route_post};
use decision_core::{DecisionEngine, ModelTier, Policy};
use gateway_core::{
  AuditError, AuditRecord, AuditSink, Gateway, GatewayConfig, Limits, ModelConfig, Secret,
};
use rig_reqwest::reqwest;
use serde_json::{Value, json};
use std::{
  collections::BTreeSet,
  sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
  },
};

pub const TOKEN: &str = "gateway-test-secret-at-least-24-bytes";
#[derive(Default)]
pub struct Audit {
  pub records: Mutex<Vec<AuditRecord>>,
  pub fail: bool,
}
#[async_trait]
impl AuditSink for Audit {
  async fn record(&self, record: AuditRecord) -> Result<(), AuditError> {
    if self.fail {
      return Err(AuditError);
    }
    self.records.lock().map_err(|_| AuditError)?.push(record);
    Ok(())
  }
}
pub struct Server {
  pub base: String,
  task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
  fn drop(&mut self) {
    self.task.abort();
  }
}
pub async fn serve(router: Router) -> Server {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("bind mock");
  let address = listener.local_addr().expect("local address");
  let task = tokio::spawn(async move {
    axum::serve(listener, router).await.expect("serve mock");
  });
  Server {
    base: format!("http://{address}"),
    task,
  }
}
pub fn model(base: &str) -> ModelConfig {
  ModelConfig {
    id: "public-model".into(),
    upstream_model: "actual-upstream-model".into(),
    api_base: format!("{base}/v1"),
    api_key: Some(Secret::new("upstream-secret")),
    api_key_env: None,
    tier: ModelTier::Balanced,
    local_inference: true,
    capabilities: BTreeSet::from(["text".into(), "tools".into()]),
    context_tokens: Some(65536),
  }
}
pub fn config(base: &str) -> GatewayConfig {
  GatewayConfig {
    token: Secret::new(TOKEN),
    models: vec![model(base)],
    limits: Limits::default(),
    constraints: Default::default(),
  }
}
pub async fn gateway(config: GatewayConfig, audit: Arc<Audit>) -> (Gateway, Server) {
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("decision engine"));
  let gateway = Gateway::new(config, engine, audit).expect("gateway");
  let server = serve(gateway.router()).await;
  (gateway, server)
}
pub fn completion(model: &str) -> Value {
  json!({"model":model,"messages":[{"role":"user","content":"hello"}],"max_tokens":32})
}
pub fn client() -> reqwest::Client {
  reqwest::Client::builder()
    .no_proxy()
    .build()
    .expect("test client")
}
pub fn post(base: &str, body: Value) -> reqwest::RequestBuilder {
  client()
    .post(format!("{base}/v1/chat/completions"))
    .bearer_auth(TOKEN)
    .json(&body)
}

#[derive(Clone, Default)]
pub struct Capture {
  pub calls: Arc<AtomicUsize>,
  pub bodies: Arc<Mutex<Vec<Value>>>,
}
pub async fn mock_json() -> (Capture, Server) {
  async fn reply(State(capture): State<Capture>, Json(body): Json<Value>) -> Json<Value> {
    capture.calls.fetch_add(1, Ordering::SeqCst);
    capture.bodies.lock().expect("capture lock").push(body);
    Json(
      json!({"id":"completion-1","object":"chat.completion","model":"actual-upstream-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}}),
    )
  }
  let capture = Capture::default();
  let server = serve(
    Router::new()
      .route("/v1/chat/completions", route_post(reply))
      .with_state(capture.clone()),
  )
  .await;
  (capture, server)
}
