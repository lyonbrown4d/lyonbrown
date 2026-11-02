use axum::{Json, Router, routing::post};
use decision_core::{DecisionEngine, ModelTier, Policy};
use gateway_core::{AuditStatus, Gateway, GatewayConfig, Limits, ModelConfig, Secret};
use gateway_storage::GatewayStorage;
use sea_orm::{Database, SqliteTransactionMode, TransactionOptions, TransactionTrait};
use serde_json::json;
use std::{
  collections::BTreeSet,
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};

const TOKEN: &str = "intent-timeout-test-access-with-at-least-32-bytes";
#[tokio::test]
async fn timed_out_pending_insert_still_reaches_terminal_without_upstream_dispatch() {
  let directory = tempfile::tempdir().expect("tempdir");
  let path = directory.path().join("gateway.sqlite");
  let store = Arc::new(GatewayStorage::open(&path).await.expect("store"));
  store
    .bootstrap_access_token(TOKEN)
    .await
    .expect("access key");
  let calls = Arc::new(AtomicUsize::new(0));
  let attempts = calls.clone();
  let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("mock listener");
  let mock_address = mock_listener.local_addr().expect("mock address");
  let mock = tokio::spawn(async move {
    axum::serve(mock_listener, Router::new().route("/v1/chat/completions", post(move || {
      attempts.fetch_add(1, Ordering::SeqCst);
      async { Json(json!({"object":"chat.completion","choices":[{"message":{"role":"assistant","content":"ok"}}]})) }
    }))).await.expect("mock server");
  });
  let settings = GatewayConfig {
    token: Secret::new(TOKEN),
    models: vec![ModelConfig {
      id: "public-model".into(),
      upstream_model: "provider-model".into(),
      api_base: format!("http://{mock_address}/v1"),
      api_key: None,
      api_key_env: None,
      tier: ModelTier::Balanced,
      local_inference: true,
      capabilities: BTreeSet::from(["text".into()]),
      context_tokens: Some(16384),
    }],
    endpoints: vec![],
    limits: Limits {
      first_byte_ms: 100,
      total_ms: 100,
      max_concurrent: 1,
      ..Default::default()
    },
    constraints: Default::default(),
  };
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("engine"));
  let gateway =
    Gateway::new_with_access(settings, engine, store.clone(), store.clone()).expect("gateway");
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("gateway listener");
  let address = listener.local_addr().expect("address");
  let router = gateway.router();
  let server = tokio::spawn(async move {
    axum::serve(listener, router).await.expect("gateway server");
  });
  let (locked, ready) = tokio::sync::oneshot::channel();
  let (release, wait) = tokio::sync::oneshot::channel();
  let writer = tokio::spawn(async move {
    let connection = Database::connect(format!("sqlite://{}", path.display()))
      .await
      .expect("writer");
    let transaction = connection
      .begin_with_options(TransactionOptions {
        sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
        ..Default::default()
      })
      .await
      .expect("lock writer");
    locked.send(()).expect("lock notification");
    tokio::time::timeout(Duration::from_secs(5), wait)
      .await
      .expect("release deadline")
      .expect("release writer");
    transaction.commit().await.expect("writer commit");
  });
  ready.await.expect("writer lock");
  let client = reqwest::Client::builder()
    .no_proxy()
    .build()
    .expect("client");
  let response = client.post(format!("http://{address}/v1/chat/completions"))
    .bearer_auth(TOKEN).json(&json!({"model":"public-model","messages":[{"role":"user","content":"hello"}],"max_tokens":32}))
    .send().await.expect("timeout response");
  assert_eq!(response.status(), 503);
  assert_eq!(calls.load(Ordering::SeqCst), 0);
  release.send(()).expect("unlock writer");
  writer.await.expect("writer task");
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let audits = store.list_audits(20, 0).await.expect("audits");
      if audits.total == 1 && matches!(audits.records[0].status, AuditStatus::Failed) {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("late Pending must receive its terminal state");
  assert_eq!(calls.load(Ordering::SeqCst), 0);
  tokio::time::timeout(Duration::from_secs(2), async {
    while gateway.observations().available_capacity != 1 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("terminal write releases admission");
  server.abort();
  mock.abort();
}
