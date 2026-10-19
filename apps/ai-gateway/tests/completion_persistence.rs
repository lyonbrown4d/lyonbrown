use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use decision_core::{DecisionEngine, ModelTier, Policy};
use gateway_core::{AuditStatus, Gateway, GatewayConfig, Limits, ModelConfig, Secret};
use gateway_storage::GatewayStorage;
use serde_json::{Value, json};
use std::{
  collections::BTreeSet,
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
};

const TOKEN: &str = "integration-access-token-with-at-least-32-bytes";
struct Server {
  base: String,
  task: Option<tokio::task::JoinHandle<()>>,
  shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}
impl Server {
  async fn stop(mut self) {
    if let Some(shutdown) = self.shutdown.take() {
      let _ = shutdown.send(());
    }
    if let Some(task) = self.task.take() {
      tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .expect("drain test connections")
        .expect("server task");
    }
  }
}
impl Drop for Server {
  fn drop(&mut self) {
    if let Some(task) = &self.task {
      task.abort();
    }
  }
}
async fn serve(router: Router) -> Server {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("listener");
  let address = listener.local_addr().expect("address");
  let (shutdown, receiver) = tokio::sync::oneshot::channel();
  let task = tokio::spawn(async move {
    axum::serve(listener, router)
      .with_graceful_shutdown(async {
        let _ = receiver.await;
      })
      .await
      .expect("server");
  });
  Server {
    base: format!("http://{address}"),
    task: Some(task),
    shutdown: Some(shutdown),
  }
}
#[derive(Clone)]
struct Capture {
  store: Arc<GatewayStorage>,
  calls: Arc<AtomicUsize>,
}
async fn upstream(
  State(capture): State<Capture>,
  headers: HeaderMap,
  Json(body): Json<Value>,
) -> Json<Value> {
  capture.calls.fetch_add(1, Ordering::SeqCst);
  assert_eq!(body["model"], "provider-model");
  let id = headers["x-request-id"].to_str().expect("request id");
  let audits = capture
    .store
    .list_audits(20, 0)
    .await
    .expect("durable intent");
  assert!(
    audits
      .records
      .iter()
      .any(|record| record.request_id == id && matches!(record.status, AuditStatus::Pending))
  );
  Json(
    json!({"id":"completion-1","object":"chat.completion","model":"provider-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"total_tokens":5}}),
  )
}
#[tokio::test]
async fn real_http_obeys_database_access_and_persists_intent_before_upstream() {
  let directory = tempfile::tempdir().expect("tempdir");
  let path = directory.path().join("gateway.sqlite");
  let store = Arc::new(GatewayStorage::open(&path).expect("store"));
  store
    .bootstrap_access_token(TOKEN)
    .await
    .expect("access token");
  let calls = Arc::new(AtomicUsize::new(0));
  let mock = serve(
    Router::new()
      .route("/v1/chat/completions", post(upstream))
      .with_state(Capture {
        store: store.clone(),
        calls: calls.clone(),
      }),
  )
  .await;
  let settings = GatewayConfig {
    token: Secret::new(TOKEN),
    models: vec![ModelConfig {
      id: "public-model".into(),
      upstream_model: "provider-model".into(),
      api_base: format!("{}/v1", mock.base),
      api_key: None,
      api_key_env: None,
      tier: ModelTier::Balanced,
      local_inference: true,
      capabilities: BTreeSet::from(["text".into()]),
      context_tokens: Some(16384),
    }],
    limits: Limits {
      max_concurrent: 1,
      ..Default::default()
    },
    constraints: Default::default(),
  };
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("engine"));
  let gateway =
    Gateway::new_with_access(settings, engine, store.clone(), store.clone()).expect("gateway");
  let server = serve(gateway.router()).await;
  let client = reqwest::Client::builder()
    .no_proxy()
    .build()
    .expect("client");
  let url = format!("{}/v1/chat/completions", server.base);
  let request =
    json!({"model":"router/auto","messages":[{"role":"user","content":"hello"}],"max_tokens":32});
  assert_eq!(
    client
      .post(&url)
      .json(&request)
      .send()
      .await
      .expect("unauthorized")
      .status(),
    401
  );
  assert_eq!(store.list_audits(20, 0).await.expect("audits").total, 0);
  assert_eq!(calls.load(Ordering::SeqCst), 0);
  let response = client
    .post(&url)
    .bearer_auth(TOKEN)
    .json(&request)
    .send()
    .await
    .expect("completion");
  assert_eq!(response.status(), 200);
  let id = response.headers()["x-request-id"]
    .to_str()
    .expect("id")
    .to_owned();
  let body: Value = response.json().await.expect("body");
  assert_eq!(body["choices"][0]["message"]["content"], "ok");
  let tokens = store.list_access_tokens().await.expect("tokens");
  store
    .revoke_access_token(&tokens[0].id)
    .await
    .expect("revoke access");
  assert_eq!(
    client
      .post(url)
      .bearer_auth(TOKEN)
      .json(&request)
      .send()
      .await
      .expect("revoked")
      .status(),
    401
  );
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  assert_eq!(store.list_audits(20, 0).await.expect("audits").total, 1);
  tokio::time::timeout(std::time::Duration::from_secs(2), async {
    while gateway.observations().available_capacity != 1 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("response body lease drained");
  drop(client);
  server.stop().await;
  drop(gateway);
  mock.stop().await;
  drop(store);
  let reopened = GatewayStorage::open(&path).expect("reopen persistent store");
  let audits = reopened.list_audits(20, 0).await.expect("persisted audit");
  assert_eq!(audits.total, 1);
  assert_eq!(audits.records[0].request_id, id);
  assert_eq!(audits.records[0].model, "public-model");
  assert_eq!(audits.records[0].policy_revision, "rules-v1");
  assert!(matches!(audits.records[0].status, AuditStatus::Succeeded));
}
