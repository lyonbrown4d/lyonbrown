use agent_inference::{ModelInfo, ModelRegistry};
use agent_protocol::v1::{self, client_service_server::ClientService};
use agent_test_support::{MockGeneration, MockInferenceBackend};
use clap::Parser;
use futures::StreamExt;
use std::sync::Arc;
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_mock_local_backend_runs_through_harness_and_persists_routing() {
  let backend = Arc::new(
    MockInferenceBackend::new(
      ModelInfo {
        id: "mock-local".into(),
        backend: "ignored-by-mock".into(),
        context_window: 4096,
        supports_tools: false,
        supports_images: false,
      },
      vec![MockGeneration::text("synthetic local result")],
    )
    .unwrap(),
  );
  let registry = Arc::new(ModelRegistry::new(1).unwrap());
  registry.register(backend.clone(), 1).await.unwrap();
  let data = tempfile::tempdir().unwrap();
  let workspace = tempfile::tempdir().unwrap();
  let mut config = agentd::config::Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(data.path().into());
  config.provider = "local".into();
  config.model = "mock-local".into();
  // This address cannot provide a remote fallback; no cloud credential is supplied.
  config.api_base = Some("http://127.0.0.1:1".into());
  let daemon = agentd::Daemon::open_with_models(config, registry)
    .await
    .unwrap();
  let session = daemon
    .create_session(Request::new(v1::CreateSessionRequest {
      title: "shared local mock".into(),
      workspace: workspace.path().to_str().unwrap().into(),
    }))
    .await
    .unwrap()
    .into_inner();
  let run = daemon
    .start_run(Request::new(v1::StartRunRequest {
      session_id: session.id,
      owner_epoch: session.owner_epoch,
      prompt: "Explain the workspace".into(),
      idempotency_key: agent_core::new_id(),
    }))
    .await
    .unwrap()
    .into_inner();
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    let mut events = daemon
      .watch_run(Request::new(v1::WatchRequest {
        run_id: run.id.clone(),
        after_cursor: 0,
      }))
      .await
      .unwrap()
      .into_inner();
    while events.next().await.is_some() {}
  })
  .await
  .unwrap();
  let result = daemon
    .get_run(Request::new(v1::RunRequest {
      run_id: run.id.clone(),
    }))
    .await
    .unwrap()
    .into_inner();
  assert_eq!(result.status, "completed", "{}", result.error);
  assert_eq!(result.result, "synthetic local result");
  let events = daemon.store.events(&run.id, 0, 1000).unwrap();
  let route = events
    .iter()
    .position(|e| e.kind == "model_route_decision")
    .unwrap();
  let plan = events
    .iter()
    .position(|e| e.kind == "execution_plan")
    .unwrap();
  let started = events.iter().position(|e| e.kind == "run_started").unwrap();
  assert!(route < plan && plan < started);
  let decision: serde_json::Value = serde_json::from_str(&events[route].payload_json).unwrap();
  assert_eq!(
    decision["output"]["value"]["candidate_model_keys"],
    serde_json::json!(["mock-local"])
  );
  let plan: serde_json::Value = serde_json::from_str(&events[plan].payload_json).unwrap();
  assert_eq!(plan["allowed_tools"], serde_json::json!([]));
  assert!(daemon.store.approvals(&run.id).unwrap().is_empty());
  let counts = backend.counts();
  assert_eq!(counts.requests, 1);
  assert_eq!(counts.completed, 1);
  daemon.shutdown().await;
}
