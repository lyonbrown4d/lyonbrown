use axum::Router;
use decision_core::{DecisionEngine, ModelTier, Policy};
use gateway_core::{AuditStatus, Gateway, GatewayConfig, ModelConfig, Secret};
use gateway_storage::GatewayStorage;
use model_mock::{AppConfig, MockApp, Protocol, Reply, Scenario};
use serde_json::json;
use std::{sync::Arc, time::Duration};

const TOKEN: &str = "mock-gateway-stream-acceptance-at-least-32-bytes";

#[tokio::test]
async fn shared_mock_success_truncation_and_post_terminal_data_have_distinct_durable_outcomes() {
  let directory = tempfile::tempdir().expect("tempdir");
  let mock = MockApp::start(AppConfig {
    listen: "127.0.0.1:0".parse().expect("loopback"),
    data_dir: directory.path().join("mock"),
    max_concurrency: 2,
    scenarios: vec![Scenario {
      model: "synthetic".into(),
      protocol: Protocol::ChatCompletions,
      repeat_last: false,
      replies: vec![
        Reply::Text {
          text: "synthetic completion".into(),
          delay_ms: 0,
        },
        Reply::Tool {
          name: "fixture_echo".into(),
          arguments: json!({"text":"synthetic"}),
          delay_ms: 0,
        },
        Reply::Truncated {
          text: "partial".into(),
        },
        Reply::Malformed {
          body: "data: {\"choices\":[{\"delta\":{}}]}\n\ndata: [DONE]\n\ndata: {\"choices\":["
            .into(),
        },
      ],
    }],
  })
  .await
  .expect("mock app");
  let store =
    Arc::new(GatewayStorage::open(directory.path().join("gateway.sqlite")).expect("store"));
  store.bootstrap_access_token(TOKEN).await.expect("access");
  store
    .save_policy(Policy::default(), None)
    .await
    .expect("policy");
  let settings = GatewayConfig {
    token: Secret::new(TOKEN),
    models: vec![ModelConfig {
      id: "public-model".into(),
      upstream_model: "synthetic".into(),
      api_base: format!("{}/v1", mock.base_url()),
      api_key: None,
      api_key_env: None,
      tier: ModelTier::Balanced,
      local_inference: true,
      capabilities: ["text".into(), "tools".into()].into(),
      context_tokens: Some(4096),
    }],
    endpoints: vec![],
    limits: Default::default(),
    constraints: Default::default(),
  };
  let engine = Arc::new(DecisionEngine::new(Policy::default(), vec![]).expect("engine"));
  let gateway =
    Gateway::new_with_access(settings, engine, store.clone(), store.clone()).expect("gateway");
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("listener");
  let address = listener.local_addr().expect("address");
  let router: Router = gateway.router();
  let server = tokio::spawn(async move {
    axum::serve(listener, router).await.expect("serve");
  });
  let client = reqwest::Client::builder()
    .no_proxy()
    .timeout(Duration::from_secs(5))
    .build()
    .expect("client");
  for index in 0..4 {
    let response = client.post(format!("http://{address}/v1/chat/completions"))
      .bearer_auth(TOKEN).json(&json!({
        "model":"public-model","stream":true,"max_tokens":32,
        "messages":[{"role":"user","content":"synthetic request"}],
        "tools":[{"type":"function","function":{"name":"fixture_echo","parameters":{"type":"object"}}}]
      })).send().await.expect("response");
    let status = response.status();
    assert_eq!(status, if index == 3 { 502 } else { 200 });
    let body = response.bytes().await;
    if index < 2 {
      let body = String::from_utf8(body.expect("complete SSE").to_vec()).expect("UTF-8");
      assert!(body.contains("data: [DONE]\n\n"));
      assert!(body.contains(if index == 0 {
        "finish_reason\":\"stop"
      } else {
        "finish_reason\":\"tool_calls"
      }));
    } else if index == 3 {
      assert!(
        body.is_ok(),
        "invalid first chunk returns a fixed HTTP error"
      );
    } else {
      assert!(
        body.is_err(),
        "truncation cannot become successful completion"
      );
    }
    tokio::time::timeout(Duration::from_secs(2), async {
      loop {
        let page = store.list_audits(10, 0).await.expect("audits");
        if page.total == index + 1
          && matches!(
            (page.records[0].status, index < 2),
            (AuditStatus::Succeeded, true) | (AuditStatus::Uncertain, false)
          )
        {
          assert_eq!(page.records[0].model, "public-model");
          assert_eq!(page.records[0].policy_revision, "rules-v1");
          break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .expect("durable terminal outcome");
  }
  let health: serde_json::Value = client
    .get(format!("{}/health", mock.base_url()))
    .send()
    .await
    .expect("health")
    .json()
    .await
    .expect("JSON");
  assert_eq!(
    health["scenarios"][0]["requests"], 4,
    "no retry of uncertain output"
  );
  server.abort();
  let _ = server.await;
  drop(client);
  drop(gateway);
  drop(store);
  drop(mock);
}
