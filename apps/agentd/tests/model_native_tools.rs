//! Actual CPU model tool calls through the sole Harness and authenticated daemon.
#![cfg(feature = "local-mistral")]
use std::{path::PathBuf, time::Duration};

use agent_client::AgentClient;
use agent_model_hub::InstallPlan;
use agent_protocol::v1;
use anyhow::{Result, ensure};
use clap::Parser;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio_stream::wrappers::TcpListenerStream;

const TEMPLATE_SHA256: &str = "5b5d4f65d0acd3b2d56a35b56d374a36cbc1c8fa5cf3b3febbbfabf22f359583";

struct Fixture {
  repository: &'static str,
  revision: &'static str,
  file: &'static str,
  template_repository: &'static str,
  template_revision: &'static str,
  root: &'static str,
  run_seconds: u64,
}

impl Fixture {
  fn selected() -> Result<Self> {
    match std::env::var("LYONBROWN_NATIVE_MODEL").as_deref() {
      Err(std::env::VarError::NotPresent) | Ok("qwen0.5b") => Ok(Self {
        repository: "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
        revision: "9217f5db79a29953eb74d5343926648285ec7e67",
        file: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
        template_repository: "Qwen/Qwen2.5-0.5B-Instruct",
        template_revision: "7ae557604adf67be50417f59c2c2f167def9a775",
        root: "model-tool-acceptance",
        run_seconds: 300,
      }),
      Ok("qwen1.5b") => Ok(Self {
        repository: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
        revision: "91cad51170dc346986eccefdc2dd33a9da36ead9",
        file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        template_repository: "Qwen/Qwen2.5-1.5B-Instruct",
        template_revision: "989aa7980e4cf806f80c7fef2b1adb7bc71aa306",
        root: "model-tool-acceptance-1.5b",
        run_seconds: 900,
      }),
      _ => anyhow::bail!("LYONBROWN_NATIVE_MODEL must be qwen0.5b or qwen1.5b"),
    }
  }
}

async fn official_template(root: &std::path::Path, fixture: &Fixture) -> Result<PathBuf> {
  let source = root.join("qwen-tokenizer-config.json");
  if !source.exists() {
    let url = format!(
      "https://huggingface.co/{}/raw/{}/tokenizer_config.json",
      fixture.template_repository, fixture.template_revision
    );
    let response = reqwest::Client::builder()
      .timeout(Duration::from_secs(60))
      .build()?
      .get(url)
      .send()
      .await?
      .error_for_status()?;
    ensure!(
      response.content_length() == Some(7305),
      "unexpected official template size"
    );
    let bytes = response.bytes().await?;
    ensure!(
      hex::encode(Sha256::digest(&bytes)) == TEMPLATE_SHA256,
      "official template hash mismatch"
    );
    std::fs::write(&source, bytes)?;
  }
  let bytes = std::fs::read(source)?;
  ensure!(
    hex::encode(Sha256::digest(&bytes)) == TEMPLATE_SHA256,
    "cached template hash mismatch"
  );
  let config: Value = serde_json::from_slice(&bytes)?;
  let template = config["chat_template"]
    .as_str()
    .ok_or_else(|| anyhow::anyhow!("official chat template absent"))?;
  let path = root.join("qwen-official.jinja");
  std::fs::write(&path, template)?;
  println!(
    "TOOLS_TEMPLATE revision={} source_sha256={TEMPLATE_SHA256} jinja_sha256={}",
    fixture.template_revision,
    hex::encode(Sha256::digest(template.as_bytes()))
  );
  Ok(path)
}

fn hub(fixture: &Fixture, action: &str, id: &str) -> v1::HubRequest {
  v1::HubRequest {
    action: action.into(),
    repository: fixture.repository.into(),
    revision: fixture.revision.into(),
    file: fixture.file.into(),
    runtime: "mistral_rs".into(),
    id: id.into(),
  }
}

async fn completed(client: &AgentClient, run: &str, seconds: u64) -> Result<v1::Run> {
  tokio::time::timeout(Duration::from_secs(seconds), async {
    loop {
      let run = client.get_run(run).await?;
      if matches!(
        run.status.as_str(),
        "completed" | "failed" | "cancelled" | "uncertain"
      ) {
        ensure!(
          run.status == "completed",
          "real model failed: {}",
          run.error
        );
        return Ok(run);
      }
      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  })
  .await?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads pinned Qwen weights and performs actual CPU tool inference"]
async fn actual_qwen_reads_workspace_and_retains_tool_result_across_turns() -> Result<()> {
  let fixture = Fixture::selected()?;
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../../.local")
    .join(fixture.root);
  std::fs::create_dir_all(&root)?;
  let root = std::fs::canonicalize(root)?;
  let workspace = tempfile::tempdir()?;
  let marker = format!("marker-{}", agent_core::new_id());
  std::fs::write(workspace.path().join("secret-marker.txt"), &marker)?;
  let mut config = agentd::config::Config::try_parse_from(["agentd"])?;
  config.data_dir = Some(root.clone());
  config.local_model_tools = true;
  config.local_model_chat_template = Some(official_template(&root, &fixture).await?);
  config.run_timeout_secs = fixture.run_seconds;
  let daemon = agentd::Daemon::open(config).await?;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let endpoint = format!("http://{}", listener.local_addr()?);
  let serving = daemon.clone();
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(agentd::client_service(serving.clone()))
      .add_service(agentd::model_service(serving))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
  });
  let result = async {
    let client = AgentClient::connect(&endpoint, daemon.auth_token()).await?;
    let plan: InstallPlan = serde_json::from_str(&client.hub_operation(hub(&fixture, "plan", "")).await?)?;
    ensure!(plan.total_bytes <= 2 * 1024 * 1024 * 1024, "acceptance artifact exceeds 2GiB");
    println!("TOOLS_MODEL_PLAN {}", serde_json::to_string(&plan)?);
    client.hub_operation(hub(&fixture, "install", &plan.id)).await?;
    tokio::time::timeout(Duration::from_secs(3600), async {
      loop {
        let status: Value = serde_json::from_str(&client.hub_operation(hub(&fixture, "status", &plan.id)).await?)?;
        match status["state"].as_str() {
          Some("installed_unverified") => return Ok::<_, anyhow::Error>(()),
          Some("failed" | "cancelled" | "uncertain") => anyhow::bail!("install failed: {status}"),
          _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
      }
    }).await??;
    if std::env::var("LYONBROWN_NATIVE_DOWNLOAD_ONLY").as_deref() == Ok("1") {
      println!("TOOLS_MODEL_DOWNLOADED {}", plan.id);
      return Ok(());
    }
    client.hub_operation(hub(&fixture, "load", &plan.id)).await?;
    let session = client.create_session("real tool model", workspace.path().to_str().ok_or_else(|| anyhow::anyhow!("workspace path"))?).await?;
    client.select_model(&session.id, session.owner_epoch, &plan.id, &format!("local:{}", plan.id)).await?;
    let run = client.start_run(&session.id, "Use the read_file tool to read secret-marker.txt. Do not guess its content. After reading, reply with the exact file content only.", &agent_core::new_id()).await?;
    let reply = completed(&client, &run.id, fixture.run_seconds).await?;
    println!("TOOLS_MODEL_REPLY {}", reply.result);
    ensure!(reply.result.contains(&marker), "model did not report actual workspace marker: {}", reply.result);
    let events = daemon.store.events(&run.id, 0, 1000)?;
    let actual_read = events.iter().filter(|event| event.kind == "tool_finished").any(|event| {
      serde_json::from_str::<Value>(&event.payload_json).is_ok_and(|value| {
        value["name"] == "read_file" && value["is_error"] == false
          && value["text"].as_str().is_some_and(|text| text.contains(&marker))
      })
    });
    ensure!(actual_read, "no persisted successful read_file result with actual marker");
    let stored = daemon.store.run(&run.id)?.ok_or_else(|| anyhow::anyhow!("run not persisted"))?;
    ensure!(stored.status == "completed" && stored.result == reply.result, "result differs from database");
    let followup = client.start_run(&session.id, "What exact marker did the file contain? Use our conversation history and repeat the marker only.", &agent_core::new_id()).await?;
    let recalled = completed(&client, &followup.id, fixture.run_seconds).await?;
    ensure!(recalled.result.contains(&marker), "tool result not retained across turns: {}", recalled.result);
    println!("TOOLS_MODEL_RECALL {}", recalled.result);
    let listing = client.start_run(&session.id, "Call list_files with exactly these arguments: {\"path\":\".\"}. The query field is forbidden. Tell me the name of the text file you find in the tool result.", &agent_core::new_id()).await?;
    let listed = completed(&client, &listing.id, fixture.run_seconds).await?;
    println!("TOOLS_MODEL_LISTING {}", listed.result);
    let listing_executed = daemon.store.events(&listing.id, 0, 1000)?.iter().filter(|event| event.kind == "tool_finished").any(|event| {
      serde_json::from_str::<Value>(&event.payload_json).is_ok_and(|value| {
        value["name"] == "list_files" && value["is_error"] == false
      })
    });
    let arithmetic = client.start_run(&session.id, "What is 2 + 2? Answer with the digit only.", &agent_core::new_id()).await?;
    let calculated = completed(&client, &arithmetic.id, fixture.run_seconds).await?;
    for turn in [&followup.id, &listing.id, &arithmetic.id] {
      let stored = daemon.store.run(turn)?.ok_or_else(|| anyhow::anyhow!("follow-up run absent"))?;
      ensure!(stored.status == "completed" && !stored.result.trim().is_empty(), "follow-up result not persisted");
      println!("TOOLS_MODEL_TURN run={turn} result={:?}", stored.result);
    }
    println!("TOOLS_MODEL_EVIDENCE run={} followup={} model={}", run.id, followup.id, plan.id);
    let mut turns = Vec::new();
    for turn in [&run.id, &followup.id, &listing.id, &arithmetic.id] {
      let stored = daemon.store.run(turn)?.ok_or_else(|| anyhow::anyhow!("evidence run absent"))?;
      let tools = daemon.store.events(turn, 0, 1000)?.into_iter().filter(|event| matches!(event.kind.as_str(), "tool_started" | "tool_finished")).map(|event| {
        Ok(serde_json::json!({"kind":event.kind,"payload":serde_json::from_str::<Value>(&event.payload_json)?}))
      }).collect::<Result<Vec<Value>>>()?;
      turns.push(serde_json::json!({"run_id":turn,"status":stored.status,"result":stored.result,"tools":tools}));
    }
    let evidence = serde_json::json!({
      "platform":"linux-x86_64", "runtime":"mistral.rs 0.8.1 CPU", "profile":"native-acceptance",
      "run_timeout_seconds":fixture.run_seconds,"model":plan, "template":{"repository":fixture.template_repository,"revision":fixture.template_revision,"source_sha256":TEMPLATE_SHA256,"jinja_sha256":"cd8e9439f0570856fd70470bf8889ebd8b5d1107207f67a5efb46e342330527f"},
      "turns":turns,"checks":{"actual_read_file":actual_read,"marker_recall":recalled.result.contains(&marker),"actual_list_files":listing_executed,"directory_summary":listed.result.contains("secret-marker.txt"),"arithmetic_exact":calculated.result.trim()=="4"},
      "limits":["single-machine CPU only","0.5B model previously emitted invalid query field and extra text","mutation approval not covered by this native test"]
    });
    std::fs::write(root.join("acceptance-evidence.json"), serde_json::to_vec_pretty(&evidence)?)?;
    ensure!(listed.result.contains("secret-marker.txt"), "model did not summarize actual directory listing: {}", listed.result);
    ensure!(listing_executed, "model did not execute list_files");
    ensure!(calculated.result.trim() == "4", "model did not follow arithmetic instruction: {}", calculated.result);
    Ok::<_, anyhow::Error>(())
  }.await;
  server.abort();
  daemon.shutdown().await;
  result
}
