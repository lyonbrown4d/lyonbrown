//! Opt-in CPU acceptance through daemon configuration, registry and the one Harness.
#![cfg(any(feature = "local-llama", feature = "local-mistral"))]
use agent_config::{LocalModelBackend, LocalModelSettings};
use agent_protocol::v1::{self, client_service_server::ClientService};
use clap::Parser;
use futures::StreamExt;
use tonic::Request;

async fn generate(backend: LocalModelBackend) -> anyhow::Result<()> {
  let data = tempfile::tempdir()?;
  let workspace = tempfile::tempdir()?;
  let mut config = agentd::config::Config::try_parse_from(["agentd"])?;
  config.data_dir = Some(data.path().into());
  config.provider = "local".into();
  config.model = "native-acceptance".into();
  config.run_timeout_secs = 180;
  config.local_models.push(LocalModelSettings {
    id: config.model.clone(),
    backend,
    path: std::env::var("LYONBROWN_TEST_GGUF")?.into(),
    context_window: 2048,
    max_output_tokens: 16,
    ..Default::default()
  });
  let daemon = agentd::Daemon::open(config).await?;
  let session = daemon
    .create_session(Request::new(v1::CreateSessionRequest {
      title: "native CPU acceptance".into(),
      workspace: workspace.path().to_string_lossy().into(),
    }))
    .await?
    .into_inner();
  let run = daemon
    .start_run(Request::new(v1::StartRunRequest {
      session_id: session.id,
      owner_epoch: session.owner_epoch,
      prompt: "Say hello briefly.".into(),
      idempotency_key: agent_core::new_id(),
    }))
    .await?
    .into_inner();
  tokio::time::timeout(std::time::Duration::from_secs(200), async {
    let mut events = daemon
      .watch_run(Request::new(v1::WatchRequest {
        run_id: run.id.clone(),
        after_cursor: 0,
      }))
      .await?
      .into_inner();
    while let Some(event) = events.next().await {
      event?;
    }
    Ok::<_, anyhow::Error>(())
  })
  .await??;
  let result = daemon
    .store
    .run(&run.id)?
    .ok_or_else(|| anyhow::anyhow!("run missing"))?;
  assert_eq!(result.status, "completed", "{}", result.error);
  assert!(!result.result.trim().is_empty());
  let events = daemon.store.events(&run.id, 0, 1000)?;
  let plan: serde_json::Value = serde_json::from_str(
    &events
      .iter()
      .find(|event| event.kind == "execution_plan")
      .ok_or_else(|| anyhow::anyhow!("plan missing"))?
      .payload_json,
  )?;
  assert_eq!(plan["allowed_tools"], serde_json::json!([]));
  assert!(events.iter().any(|event| event.kind == "model_selected"));
  assert!(events.iter().any(|event| event.kind == "run_started"));
  assert_eq!(daemon.local_models().len(), 1);
  assert!(!daemon.local_models()[0].supports_tools);
  daemon.shutdown().await;
  Ok(())
}
#[cfg(feature = "local-llama")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires LYONBROWN_TEST_GGUF pointing to an existing supported GGUF"]
async fn llama_cpu_daemon_generates() -> anyhow::Result<()> {
  generate(LocalModelBackend::Llama).await
}
#[cfg(feature = "local-mistral")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires LYONBROWN_TEST_GGUF pointing to an existing supported GGUF"]
async fn mistral_cpu_daemon_generates() -> anyhow::Result<()> {
  generate(LocalModelBackend::Mistral).await
}
