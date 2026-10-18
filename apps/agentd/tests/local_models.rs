mod local_support;
use agent_inference::*;
use agent_protocol::v1::{self, client_service_server::ClientService};
use clap::Parser;
use futures::StreamExt;
use local_support::Backend;
use serde_json::json;
use std::sync::Arc;
use tonic::Request;

async fn open(models: &[Arc<Backend>], decision: bool) -> (tempfile::TempDir, agentd::Daemon) {
  let data = tempfile::tempdir().unwrap();
  let mut config = agentd::config::Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(data.path().into());
  config.provider = "local".into();
  config.model = "main".into();
  // An invalid cloud address and no credentials expose an accidental remote fallback.
  config.api_base = Some("http://127.0.0.1:1".into());
  config.decision.model = decision.then(|| "classifier".into());
  let registry = Arc::new(ModelRegistry::new(2).unwrap());
  for model in models {
    registry.register(model.clone(), 1).await.unwrap();
  }
  let daemon = agentd::Daemon::open_with_models(config, registry)
    .await
    .unwrap();
  (data, daemon)
}
async fn run(daemon: &agentd::Daemon, workspace: &std::path::Path) -> v1::Run {
  let session = daemon
    .create_session(Request::new(v1::CreateSessionRequest {
      title: "local models".into(),
      workspace: workspace.to_str().unwrap().into(),
    }))
    .await
    .unwrap()
    .into_inner();
  let run = daemon
    .start_run(Request::new(v1::StartRunRequest {
      session_id: session.id,
      owner_epoch: session.owner_epoch,
      prompt: "explain the workspace".into(),
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
  daemon
    .get_run(Request::new(v1::RunRequest { run_id: run.id }))
    .await
    .unwrap()
    .into_inner()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_uses_registered_model_and_persists_tool_free_plan() {
  let backend = Backend::new("main", false, Some("local result"));
  let (_data, daemon) = open(std::slice::from_ref(&backend), false).await;
  let workspace = tempfile::tempdir().unwrap();
  let run = run(&daemon, workspace.path()).await;
  assert_eq!(run.status, "completed", "{}", run.error);
  assert_eq!(run.result, "local result");
  let requests = backend.requests.lock().await;
  assert_eq!(requests.len(), 1);
  assert!(requests[0].tools.is_empty());
  assert!(requests[0].max_tokens < backend.info().context_window);
  let events = daemon.store.events(&run.id, 0, 1000).unwrap();
  let event = |kind| {
    serde_json::from_str::<serde_json::Value>(
      &events
        .iter()
        .find(|event| event.kind == kind)
        .unwrap()
        .payload_json,
    )
    .unwrap()
  };
  assert_eq!(event("execution_plan")["allowed_tools"], json!([]));
  assert_eq!(event("execution_plan")["required_capabilities"], json!([]));
  assert_eq!(event("model_selected")["supports_tools"], false);
  assert_eq!(daemon.local_models()[0].backend, "synthetic");
  assert!(daemon.store.approvals(&run.id).unwrap().is_empty());
  daemon.shutdown().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn decision_local_profile_is_independent_and_only_reduces_execution_authority() {
  for supports_tools in [true, false] {
    let route = json!({"task_kind":"code","model_policy":"general","allowed_tools":["read_file"],"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.99});
    let main = Backend::new("main", supports_tools, Some("main output"));
    let classifier = Backend::new("classifier", false, Some(&route.to_string()));
    let (_data, daemon) = open(&[main.clone(), classifier.clone()], true).await;
    let workspace = tempfile::tempdir().unwrap();
    let run = run(&daemon, workspace.path()).await;
    assert_eq!(run.status, "completed", "{}", run.error);
    assert_eq!(run.result, "main output");
    let requests = main.requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].tools.len(), usize::from(supports_tools));
    if supports_tools {
      assert_eq!(requests[0].tools[0].name, "read_file");
    }
    let decisions = classifier.requests.lock().await;
    assert_eq!(decisions.len(), 1);
    assert!(decisions[0].tools.is_empty());
    let events = daemon.store.events(&run.id, 0, 1000).unwrap();
    let plan: serde_json::Value = serde_json::from_str(
      &events
        .iter()
        .find(|event| event.kind == "execution_plan")
        .unwrap()
        .payload_json,
    )
    .unwrap();
    assert_eq!(plan["source"]["kind"], "classifier");
    assert_eq!(
      plan["allowed_tools"],
      if supports_tools {
        json!(["read_file"])
      } else {
        json!([])
      }
    );
    assert_eq!(plan["source"]["restricted"], !supports_tools);
    assert!(!workspace.path().join("forbidden.txt").exists());
    daemon.shutdown().await;
  }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_local_model_and_uncompiled_backend_fail_before_daemon_is_ready() {
  let data = tempfile::tempdir().unwrap();
  let mut config = agentd::config::Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(data.path().into());
  config.provider = "local".into();
  config.model = "missing".into();
  let registry = Arc::new(ModelRegistry::new(1).unwrap());
  assert!(
    agentd::Daemon::open_with_models(config.clone(), registry)
      .await
      .is_err()
  );
  if !cfg!(feature = "local-llama") {
    config.local_models.push(agent_config::LocalModelSettings {
      id: "missing".into(),
      path: data.path().join("missing.gguf"),
      ..Default::default()
    });
    let error = match agentd::Daemon::open(config).await {
      Err(error) => error,
      Ok(_) => panic!("uncompiled backend was accepted"),
    };
    assert!(error.to_string().contains("build with local-llama"));
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_model_cancellation_persists_cancelled_for_classifier_and_harness() {
  for during_classification in [true, false] {
    let main = Backend::new("main", true, None);
    let classifier = Backend::new("classifier", false, None);
    let (_data, daemon) = open(&[main.clone(), classifier.clone()], during_classification).await;
    let workspace = tempfile::tempdir().unwrap();
    let session = daemon
      .create_session(Request::new(v1::CreateSessionRequest {
        title: "cancel local classification".into(),
        workspace: workspace.path().to_str().unwrap().into(),
      }))
      .await
      .unwrap()
      .into_inner();
    let run = daemon
      .start_run(Request::new(v1::StartRunRequest {
        session_id: session.id,
        owner_epoch: session.owner_epoch,
        prompt: "cancel during classification".into(),
        idempotency_key: agent_core::new_id(),
      }))
      .await
      .unwrap()
      .into_inner();
    tokio::time::timeout(
      std::time::Duration::from_secs(2),
      if during_classification {
        classifier.started.notified()
      } else {
        main.started.notified()
      },
    )
    .await
    .unwrap();
    daemon
      .cancel_run(Request::new(v1::RunRequest {
        run_id: run.id.clone(),
      }))
      .await
      .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
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
    assert_eq!(
      daemon.store.run(&run.id).unwrap().unwrap().status,
      "cancelled"
    );
    assert_eq!(main.requests.lock().await.is_empty(), during_classification);
    assert_eq!(
      daemon
        .store
        .events(&run.id, 0, 1000)
        .unwrap()
        .iter()
        .any(|event| event.kind == "run_started"),
      !during_classification
    );
    daemon.shutdown().await;
  }
}
