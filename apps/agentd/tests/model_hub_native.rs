//! Explicit opt-in: downloads real public weights and performs CPU inference through RPC.
#![cfg(feature = "local-llama")]

use std::{path::PathBuf, time::Duration};

use agent_client::AgentClient;
use agent_model_hub::InstallPlan;
use agent_protocol::v1;
use anyhow::{Result, ensure};
use clap::Parser;
use serde_json::Value;
use tokio_stream::wrappers::TcpListenerStream;

const REPOSITORY: &str = "QuantFactory/SmolLM2-135M-Instruct-GGUF";
const REVISION: &str = "476854d00ede130660aba430d15f9347ad2e7d0e";
const FILE: &str = "SmolLM2-135M-Instruct.Q4_K_M.gguf";

fn request(action: &str, id: &str) -> v1::HubRequest {
  v1::HubRequest {
    action: action.into(),
    repository: std::env::var("LYONBROWN_ACCEPTANCE_HF_REPO").unwrap_or_else(|_| REPOSITORY.into()),
    revision: std::env::var("LYONBROWN_ACCEPTANCE_HF_REVISION").unwrap_or_else(|_| REVISION.into()),
    file: std::env::var("LYONBROWN_ACCEPTANCE_HF_FILE").unwrap_or_else(|_| FILE.into()),
    runtime: "llama_cpp".into(),
    id: id.into(),
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads ~105 MB public HF model and runs real llama.cpp CPU inference"]
async fn downloads_loads_selects_and_converses_through_authenticated_agentd() -> Result<()> {
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/model-acceptance");
  std::fs::create_dir_all(&root)?;
  let root = std::fs::canonicalize(root)?;
  let workspace = tempfile::tempdir()?;
  let mut config = agentd::config::Config::try_parse_from(["agentd"])?;
  config.data_dir = Some(root.clone());
  config.run_timeout_secs = 180;
  let daemon = agentd::Daemon::open(config).await?;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let endpoint = format!("http://{}", listener.local_addr()?);
  let server_daemon = daemon.clone();
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(agentd::client_service(server_daemon.clone()))
      .add_service(agentd::model_service(server_daemon))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
  });
  let result = async {
    let client = AgentClient::connect(&endpoint, daemon.auth_token()).await?;
    let session = client
      .create_session(
        "real model Hub acceptance",
        &workspace.path().to_string_lossy(),
      )
      .await?;
    let plan: InstallPlan =
      serde_json::from_str(&client.hub_operation(request("plan", "")).await?)?;
    ensure!(
      plan.revision == request("plan", "").revision,
      "revision changed"
    );
    ensure!(
      plan.total_bytes <= 512 * 1024 * 1024,
      "acceptance model exceeds 512 MB budget"
    );
    println!("MODEL_PLAN {}", serde_json::to_string(&plan)?);
    client.hub_operation(request("install", &plan.id)).await?;
    let status = tokio::time::timeout(Duration::from_secs(900), async {
      loop {
        let status: Value =
          serde_json::from_str(&client.hub_operation(request("status", &plan.id)).await?)?;
        match status["state"].as_str() {
          Some("installed_unverified") => return Ok::<_, anyhow::Error>(status),
          Some("failed" | "cancelled" | "uncertain") => {
            anyhow::bail!("model installation failed: {status}")
          }
          _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
      }
    })
    .await??;
    ensure!(
      status["model"]["verified_files"][0]["sha256"]
        .as_str()
        .is_some(),
      "missing artifact hash"
    );
    println!("MODEL_INSTALL {}", serde_json::to_string(&status)?);
    if plan.repository == REPOSITORY && plan.revision == REVISION {
      ensure!(
        status["model"]["verified_files"][0]["sha256"]
          == "8030f04528538d47bda434f6f0bdf3952c40a58123e4d5e755332f23731a8684",
        "download hash differs from pinned published artifact"
      );
    }
    client.hub_operation(request("load", &plan.id)).await?;
    let models = client.list_models(&session.id).await?;
    let model = models
      .iter()
      .find(|model| model.id == plan.id)
      .ok_or_else(|| anyhow::anyhow!("loaded model absent from unified catalog"))?;
    ensure!(
      model.state == "ready",
      "loaded model not ready: {}",
      model.state
    );
    let execution = format!("local:{}", plan.id);
    client
      .select_model(&session.id, session.owner_epoch, &plan.id, &execution)
      .await?;
    let run = client
      .start_run(
        &session.id,
        "Say hello briefly in one sentence.",
        &agent_core::new_id(),
      )
      .await?;
    let completed = tokio::time::timeout(Duration::from_secs(200), async {
      loop {
        let current = client.get_run(&run.id).await?;
        if matches!(
          current.status.as_str(),
          "completed" | "failed" | "cancelled" | "uncertain"
        ) {
          return Ok::<_, anyhow::Error>(current);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
      }
    })
    .await??;
    ensure!(
      completed.status == "completed",
      "real inference failed: {}",
      completed.error
    );
    ensure!(
      !completed.result.trim().is_empty(),
      "empty inference response"
    );
    let stored = daemon
      .store
      .run(&run.id)?
      .ok_or_else(|| anyhow::anyhow!("run missing"))?;
    ensure!(
      stored.status == "completed" && stored.result == completed.result,
      "result not persisted"
    );
    let selection = daemon
      .store
      .model_binding(&session.id)?
      .ok_or_else(|| anyhow::anyhow!("model selection not persisted"))?;
    ensure!(selection.model_id == plan.id, "persisted selection differs");
    println!("MODEL_RESPONSE {}", completed.result);
    // Preserve one Session across turns; these prompts never repeat the marker.
    for prompt in [
      "Remember this code word: violet. Reply with the code word only.",
      "What code word did I ask you to remember? Reply with the word only.",
      "Count from one to three using digits only.",
    ] {
      let turn = client
        .start_run(&session.id, prompt, &agent_core::new_id())
        .await?;
      let reply = tokio::time::timeout(Duration::from_secs(200), async {
        loop {
          let reply = client.get_run(&turn.id).await?;
          if matches!(
            reply.status.as_str(),
            "completed" | "failed" | "cancelled" | "uncertain"
          ) {
            return Ok::<_, anyhow::Error>(reply);
          }
          tokio::time::sleep(Duration::from_millis(50)).await;
        }
      })
      .await??;
      ensure!(
        reply.status == "completed",
        "multi-turn inference failed: {}",
        reply.error
      );
      ensure!(!reply.result.trim().is_empty(), "empty multi-turn output");
      if prompt.starts_with("What code word") {
        ensure!(
          reply.result.to_ascii_lowercase().contains("violet"),
          "model did not retain conversation marker: {}",
          reply.result
        );
      }
      let persisted = daemon
        .store
        .run(&turn.id)?
        .ok_or_else(|| anyhow::anyhow!("turn missing"))?;
      ensure!(
        persisted.result == reply.result && persisted.status == "completed",
        "turn not persisted"
      );
      let events = daemon.store.events(&turn.id, 0, 1000)?;
      let plan = events
        .iter()
        .find(|event| event.kind == "execution_plan")
        .ok_or_else(|| anyhow::anyhow!("execution plan absent"))?;
      let plan: Value = serde_json::from_str(&plan.payload_json)?;
      ensure!(
        plan["allowed_tools"] == serde_json::json!([]),
        "tool-free model gained tool authority"
      );
      println!(
        "MODEL_TURN prompt={prompt:?} response={:?} run={}",
        reply.result, turn.id
      );
    }
    println!(
      "MODEL_EVIDENCE run={} model={} directory={}",
      run.id,
      plan.id,
      root.display()
    );
    Ok::<_, anyhow::Error>(())
  }
  .await;
  server.abort();
  daemon.shutdown().await;
  result
}
