use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_client::{NativeAgentClient, RemoteClientOptions};
use agent_launch::{
  CommandDaemonSpawner, CoordinatorTargetResolver, GrpcReadinessProbe, LaunchCoordinator,
  LaunchRequest, LocalDiscoverySource, canonicalize_optional_path, scoped_run_directory,
};
use agent_runtime::config::{CliOverrides, LoadOptions, RuntimeConfig};
use anyhow::{Context, Result};
use clap::Parser;

const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CONNECTION_ERROR_CHARS: usize = 512;

#[derive(Debug, Clone, Parser)]
#[command(name = "agent-gui", version, about = "Lyonbrown agent desktop client")]
pub struct GuiArgs {
  /// Workspace served by the local agent daemon.
  #[arg(default_value = ".", value_name = "WORKSPACE")]
  pub workspace: PathBuf,

  /// Explicit configuration file (TOML, YAML, JSON, JSON5, RON, or INI).
  #[arg(long)]
  pub config: Option<PathBuf>,

  #[arg(long)]
  pub provider: Option<String>,

  #[arg(long)]
  pub model: Option<String>,

  /// Agent profile ID from ~/.agent/profiles.
  #[arg(long)]
  pub profile: Option<String>,

  /// Ollama base URL.
  #[arg(long)]
  pub endpoint: Option<String>,

  /// Allow all tool requests for this daemon session.
  #[arg(long, conflicts_with = "deny_all")]
  pub approve_all: bool,

  /// Deny all write, execute, and network tool requests.
  #[arg(long, conflicts_with = "approve_all")]
  pub deny_all: bool,

  /// Trust MCP commands declared by the workspace configuration.
  #[arg(long)]
  pub trust_workspace_mcp: bool,

  /// Trust model, approval, and storage workspace settings.
  #[arg(long)]
  pub trust_workspace_config: bool,
}

pub async fn connect_native_client(args: &GuiArgs) -> Result<NativeAgentClient> {
  let mut args = args.clone();
  let workspace = std::fs::canonicalize(&args.workspace).with_context(|| {
    format!(
      "failed to resolve workspace {}",
      args.workspace.to_string_lossy()
    )
  })?;
  let config_display = args.config.as_ref().map(|path| path.display().to_string());
  args.config = canonicalize_optional_path(args.config.as_deref()).with_context(|| {
    format!(
      "failed to resolve configuration {}",
      config_display.as_deref().unwrap_or("<none>")
    )
  })?;
  let config = load_runtime_config(&args, &workspace)?;
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
      daemon_fixed_arguments(&args),
    )),
    Arc::new(GrpcReadinessProbe::default()),
  ));
  let resolver = Arc::new(CoordinatorTargetResolver::new(coordinator, request));
  NativeAgentClient::start_with_resolver(
    resolver,
    uuid::Uuid::new_v4().to_string(),
    RemoteClientOptions::default(),
  )
  .context("failed to start the GUI daemon client")
}

fn load_runtime_config(args: &GuiArgs, workspace: &Path) -> Result<RuntimeConfig> {
  let mut options = LoadOptions::for_workspace(workspace);
  options.explicit_config = args.config.clone();
  options.cli = CliOverrides {
    provider: args.provider.clone(),
    model: args.model.clone(),
    profile: args.profile.clone(),
    endpoint: args.endpoint.clone(),
    approve_all: args.approve_all.then_some(true),
    deny_all: args.deny_all.then_some(true),
    trust_workspace_mcp: args.trust_workspace_mcp,
    trust_workspace_config: args.trust_workspace_config,
    ..Default::default()
  };
  RuntimeConfig::load(options).context("failed to load GUI runtime configuration")
}

pub fn sibling_agentd_path(gui_executable: &Path) -> Result<PathBuf> {
  let directory = gui_executable
    .parent()
    .context("GUI executable has no parent directory")?;
  let name = if cfg!(windows) {
    "agentd.exe"
  } else {
    "agentd"
  };
  Ok(directory.join(name))
}

pub fn daemon_fixed_arguments(args: &GuiArgs) -> Vec<OsString> {
  let mut arguments = Vec::new();
  push_path(&mut arguments, "--config", args.config.as_deref());
  push_string(&mut arguments, "--provider", args.provider.as_deref());
  push_string(&mut arguments, "--model", args.model.as_deref());
  push_string(&mut arguments, "--profile", args.profile.as_deref());
  push_string(&mut arguments, "--endpoint", args.endpoint.as_deref());
  push_flag(&mut arguments, "--approve-all", args.approve_all);
  push_flag(&mut arguments, "--deny-all", args.deny_all);
  push_flag(
    &mut arguments,
    "--trust-workspace-mcp",
    args.trust_workspace_mcp,
  );
  push_flag(
    &mut arguments,
    "--trust-workspace-config",
    args.trust_workspace_config,
  );
  arguments
}

pub fn sanitize_connection_error(error: &str) -> String {
  let sanitized: String = error
    .chars()
    .map(|character| {
      if character.is_control() {
        ' '
      } else {
        character
      }
    })
    .take(MAX_CONNECTION_ERROR_CHARS)
    .collect();
  let sanitized = sanitized.trim();
  if sanitized.is_empty() {
    "agentd connection failed".to_owned()
  } else {
    sanitized.to_owned()
  }
}

fn push_path(arguments: &mut Vec<OsString>, flag: &str, value: Option<&Path>) {
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
