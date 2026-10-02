use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_client::{NativeAgentClient, RemoteClientOptions};
use agent_launch::{
  CommandDaemonSpawner, CoordinatorTargetResolver, GrpcReadinessProbe, LaunchCoordinator,
  LaunchRequest, LocalDiscoverySource, canonicalize_optional_path, scoped_run_directory,
};
use anyhow::{Context, Result};
use clap::Parser;

use crate::config::{CliOverrides, LoadOptions, RuntimeConfig};

const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Parser)]
#[command(name = "agent", version, about = "Local-first agent runtime")]
pub struct Cli {
  /// Workspace served by the local agent daemon.
  #[arg(default_value = ".", value_name = "WORKSPACE")]
  pub workspace: PathBuf,

  /// Explicit configuration file (TOML, YAML, JSON, JSON5, RON, or INI).
  #[arg(long, global = true)]
  pub config: Option<PathBuf>,

  #[arg(long, global = true)]
  pub provider: Option<String>,

  #[arg(long, global = true)]
  pub model: Option<String>,

  /// Agent profile ID from ~/.agent/profiles.
  #[arg(long, global = true)]
  pub profile: Option<String>,

  /// Ollama base URL.
  #[arg(long, global = true)]
  pub endpoint: Option<String>,

  /// Allow all tool requests for this session.
  #[arg(long, global = true, conflicts_with = "deny_all")]
  pub approve_all: bool,

  /// Deny all write, execute, and network tool requests for this session.
  #[arg(long, global = true, conflicts_with = "approve_all")]
  pub deny_all: bool,

  /// Trust MCP server commands defined by the workspace agent.toml.
  #[arg(long, global = true)]
  pub trust_workspace_mcp: bool,

  /// Trust model, approval, and storage settings defined by workspace agent.toml.
  #[arg(long, global = true)]
  pub trust_workspace_config: bool,
}

pub async fn run(mut cli: Cli) -> Result<()> {
  let workspace = std::fs::canonicalize(&cli.workspace).with_context(|| {
    format!(
      "failed to resolve workspace {}",
      cli.workspace.to_string_lossy()
    )
  })?;
  let config_display = cli.config.as_ref().map(|path| path.display().to_string());
  cli.config = canonicalize_optional_path(cli.config.as_deref()).with_context(|| {
    format!(
      "failed to resolve configuration {}",
      config_display.as_deref().unwrap_or("<none>")
    )
  })?;
  let mut options = LoadOptions::for_workspace(&workspace);
  options.explicit_config = cli.config.clone();
  options.cli = CliOverrides {
    provider: cli.provider.clone(),
    model: cli.model.clone(),
    profile: cli.profile.clone(),
    endpoint: cli.endpoint.clone(),
    approve_all: cli.approve_all.then_some(true),
    deny_all: cli.deny_all.then_some(true),
    trust_workspace_mcp: cli.trust_workspace_mcp,
    trust_workspace_config: cli.trust_workspace_config,
    ..Default::default()
  };
  let config = RuntimeConfig::load(options)?;
  let home = dirs::home_dir().context("home directory is unavailable")?;
  let request = LaunchRequest::new(
    workspace.clone(),
    config.fingerprint()?,
    true,
    DAEMON_START_TIMEOUT,
  )?;
  let run_directory = scoped_run_directory(&home, &workspace, request.scope_fingerprint())?;
  let agentd = sibling_agentd_path(&std::env::current_exe()?)?;
  let coordinator = Arc::new(LaunchCoordinator::new(
    Arc::new(LocalDiscoverySource::new(run_directory)),
    Arc::new(CommandDaemonSpawner::new(
      agentd,
      daemon_fixed_arguments(&cli),
    )),
    Arc::new(GrpcReadinessProbe::default()),
  ));
  let resolver = Arc::new(CoordinatorTargetResolver::new(coordinator, request));
  let client = NativeAgentClient::start_with_resolver(
    resolver,
    uuid::Uuid::new_v4().to_string(),
    RemoteClientOptions::default(),
  )?;
  crate::tui::run_native_client(client).await
}

pub fn sibling_agentd_path(agent_executable: &Path) -> Result<PathBuf> {
  let directory = agent_executable
    .parent()
    .context("agent executable has no parent directory")?;
  let name = if cfg!(windows) {
    "agentd.exe"
  } else {
    "agentd"
  };
  Ok(directory.join(name))
}

pub fn daemon_fixed_arguments(cli: &Cli) -> Vec<OsString> {
  let mut arguments = Vec::new();
  push_value(&mut arguments, "--config", cli.config.as_deref());
  push_string(&mut arguments, "--provider", cli.provider.as_deref());
  push_string(&mut arguments, "--model", cli.model.as_deref());
  push_string(&mut arguments, "--profile", cli.profile.as_deref());
  push_string(&mut arguments, "--endpoint", cli.endpoint.as_deref());
  push_flag(&mut arguments, "--approve-all", cli.approve_all);
  push_flag(&mut arguments, "--deny-all", cli.deny_all);
  push_flag(
    &mut arguments,
    "--trust-workspace-mcp",
    cli.trust_workspace_mcp,
  );
  push_flag(
    &mut arguments,
    "--trust-workspace-config",
    cli.trust_workspace_config,
  );
  arguments
}

fn push_value(arguments: &mut Vec<OsString>, flag: &str, value: Option<&Path>) {
  if let Some(value) = value {
    arguments.push(flag.into());
    arguments.push(value.as_os_str().to_owned());
  }
}

fn push_string(arguments: &mut Vec<OsString>, flag: &str, value: Option<&str>) {
  if let Some(value) = value {
    arguments.push(flag.into());
    arguments.push(value.into());
  }
}

fn push_flag(arguments: &mut Vec<OsString>, flag: &str, enabled: bool) {
  if enabled {
    arguments.push(flag.into());
  }
}
