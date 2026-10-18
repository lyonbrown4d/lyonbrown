use agent_dev::{DevelopmentDaemon, Options, Plan, ProductBuild, Target, client_command};
use anyhow::{Context, Result};
use clap::Parser;
use std::{collections::BTreeMap, path::Path, process::ExitCode, time::Duration};

#[tokio::main]
async fn main() -> Result<ExitCode> {
  let options = Options::parse();
  let root = Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .canonicalize()
    .context("locate repository root")?;
  let mut plan = Plan::new(&root, &options)?;
  eprintln!("Development data: {}", plan.data_dir.display());
  eprintln!("Development endpoint: {}", plan.endpoint());
  eprintln!(
    "Configuration: {}",
    plan
      .config
      .clone()
      .unwrap_or_else(|| plan.data_dir.join("config.toml"))
      .display()
  );
  eprintln!("{}", ProductBuild::Daemon.display_command());
  if plan.target != Target::Daemon {
    eprintln!("{}", plan.product_build().display_command());
  }
  if options.dry_run {
    return Ok(ExitCode::SUCCESS);
  }
  let ambient: BTreeMap<String, String> = std::env::vars_os()
    .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
    .collect();
  plan.prepare(&ambient)?;
  let settings = agent_config::load_with_environment(
    &plan.data_dir,
    plan.config.as_deref(),
    &plan.environment(&ambient),
  )?;
  let _logs = agent_telemetry::init_with_options(
    &agent_telemetry::LogOptions {
      directory: settings.log_directory,
      retained_files: settings.log_retention,
      stderr: false,
    },
    "agent-dev",
    ambient
      .get("RUST_LOG")
      .map(String::as_str)
      .unwrap_or("info"),
  )?;
  tracing::info!(operation = "dev.launch", outcome = "started", process_id = std::process::id(), target = ?plan.target, endpoint = %plan.endpoint(), "Development launcher started");
  let result = launch(&root, &plan, &ambient).await;
  if let Err(error) = &result {
    tracing::error!(operation = "dev.launch", outcome = "failed", error = %error, recovery = "Inspect the selected development logs and the owning terminal", "Development launch failed");
  }
  result
}

async fn launch(root: &Path, plan: &Plan, ambient: &BTreeMap<String, String>) -> Result<ExitCode> {
  let binary = tokio::select! {
    _ = tokio::signal::ctrl_c() => anyhow::bail!("development build cancelled"),
    binary = ProductBuild::Daemon.build(root) => binary?,
  };
  let mut daemon =
    DevelopmentDaemon::start(root, plan, &binary, ambient, Duration::from_secs(60)).await?;
  tracing::info!(operation = "dev.daemon", outcome = if daemon.is_owner() { "started" } else { "reused" }, process_id = std::process::id(), endpoint = %plan.endpoint(), "Development daemon is ready");
  if daemon.is_owner() {
    eprintln!(
      "Started development agentd. This terminal supervises it after the client closes; Ctrl+C stops it."
    );
  } else {
    eprintln!("Reusing verified development agentd; its original terminal supervises it.");
  }
  let result = run(root, plan, ambient, &mut daemon).await;
  daemon.stop().await?;
  tracing::info!(
    operation = "dev.launch",
    outcome = "stopped",
    process_id = std::process::id(),
    "Development launcher stopped"
  );
  result
}
async fn run(
  root: &Path,
  plan: &Plan,
  ambient: &BTreeMap<String, String>,
  daemon: &mut DevelopmentDaemon,
) -> Result<ExitCode> {
  let mut exit = ExitCode::SUCCESS;
  if plan.target != Target::Daemon {
    if plan.target == Target::Desktop {
      eprintln!(
        "Default Desktop build has no embedded CEF runtime; see packaging/README.md for native bundles."
      );
    }
    let binary = tokio::select! {
      _ = tokio::signal::ctrl_c() => return Ok(ExitCode::FAILURE),
      result = daemon.wait() => return result.map(|()| ExitCode::FAILURE),
      binary = plan.product_build().build(root) => binary?,
    };
    let mut command = tokio::process::Command::from(client_command(
      &binary,
      root,
      plan,
      ambient,
      &daemon.connection,
    ));
    command.kill_on_drop(true);
    // No pipe or process group: TUI receives the user's terminal and normal foreground input.
    let mut client = command.spawn().context("launch development client")?;
    let status = tokio::select! {
      _ = tokio::signal::ctrl_c() => { let _ = client.kill().await; return Ok(ExitCode::FAILURE); },
      result = daemon.wait() => return result.map(|()| ExitCode::FAILURE),
      status = client.wait() => status?,
    };
    exit = status
      .code()
      .and_then(|code| u8::try_from(code).ok())
      .map(ExitCode::from)
      .unwrap_or(ExitCode::FAILURE);
  }
  if daemon.is_owner() {
    eprintln!(
      "Development agentd remains active. Keep this terminal open; Ctrl+C stops it and its development jobs."
    );
    tokio::select! {
      _ = tokio::signal::ctrl_c() => {},
      result = daemon.wait() => result?,
    }
  }
  Ok(exit)
}
