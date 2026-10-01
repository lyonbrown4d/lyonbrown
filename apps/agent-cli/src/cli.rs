use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_core::SessionId;
use agent_mcp::McpBridge;
#[cfg(feature = "llama-cpp")]
use agent_model::ModelTaskRouter;
use agent_model::{ModelRegistry, TaskRouter};
use agent_profile::{AgentProfile, ProfileError, ProfileLoader};
use agent_secret::{CompositeBroker, SecretBroker};
use agent_skill::SkillRegistry;
use agent_storage::SessionStore;
use agent_tool::{
  ListFilesTool, ReadFileTool, SearchWorkspaceTool, SessionApprovalPolicy, ShellTool, ToolRegistry,
  WriteFileTool,
};
use agent_workspace::WorkspaceProfile;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use provider_anthropic::AnthropicProvider;
#[cfg(feature = "llama-cpp")]
use provider_llamacpp::{GgufModel, LlamaCppConfig, LlamaCppProvider};
use provider_ollama::OllamaProvider;
use provider_openai::OpenAiProvider;

use crate::approval::policy_from_config;
use crate::config::{CliOverrides, LoadOptions, RuntimeConfig};
use agent_app::approval_channel;

#[derive(Debug, Parser)]
#[command(name = "agent", version, about = "Local-first agent runtime")]
pub struct Cli {
  #[command(subcommand)]
  pub command: Option<Command>,

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

impl Cli {
  pub fn take_command_or_default(&mut self) -> Command {
    self.command.take().unwrap_or_else(|| Command::Tui {
      workspace: PathBuf::from("."),
    })
  }
}

#[derive(Debug, Subcommand)]
pub enum Command {
  /// Ask one question and stream the answer.
  Ask { prompt: String },
  /// Start a multi-turn terminal chat.
  Chat,
  /// Continue a stored session in the terminal chat.
  Resume { session_id: String },
  /// List stored sessions.
  Sessions,
  /// Open the workspace terminal interface.
  #[command(name = ".")]
  Tui {
    #[arg(skip = PathBuf::from("."))]
    workspace: PathBuf,
  },
}

pub async fn run(mut cli: Cli) -> Result<()> {
  let command = cli.take_command_or_default();
  let workspace = match &command {
    Command::Tui { workspace } => workspace.clone(),
    _ => std::env::current_dir().context("failed to determine workspace directory")?,
  };
  let mut options = LoadOptions::for_workspace(&workspace);
  options.explicit_config = cli.config;
  options.cli = CliOverrides {
    provider: cli.provider,
    model: cli.model,
    profile: cli.profile,
    endpoint: cli.endpoint,
    approve_all: cli.approve_all.then_some(true),
    deny_all: cli.deny_all.then_some(true),
    trust_workspace_mcp: cli.trust_workspace_mcp,
    trust_workspace_config: cli.trust_workspace_config,
    ..Default::default()
  };
  let config = RuntimeConfig::load(options)?;
  #[cfg(feature = "llama-cpp")]
  let mut config = config;
  let home = dirs::home_dir().context("home directory is unavailable")?;
  if matches!(command, Command::Sessions) {
    let store = SessionStore::open(storage_path(&config, &workspace, &home)).await?;
    for session in store.sessions().await? {
      println!(
        "{}\t{}\t{}",
        session.id, session.updated_at, session.event_count
      );
    }
    return Ok(());
  }
  #[cfg(feature = "llama-cpp")]
  if config.model.provider == "llamacpp"
    && config.llamacpp.model_path.is_none()
    && matches!(&command, Command::Tui { .. })
  {
    let Some(prepared) =
      crate::tui::prepare_embedded_model(&home, config.model.model.as_deref()).await?
    else {
      return Ok(());
    };
    config.model.model = Some(prepared.id.to_owned());
    config.llamacpp.model_path = Some(prepared.path);
  }
  let (approval, tui_approvals) = match &command {
    Command::Tui { .. } if !config.approval.approve_all && !config.approval.deny_all => {
      let (prompt, requests) = approval_channel(4);
      (
        SessionApprovalPolicy::interactive_with_timeout(
          Arc::new(prompt),
          std::time::Duration::from_secs(config.approval.timeout_seconds),
        ),
        Some(requests),
      )
    }
    _ => (policy_from_config(&config.approval), None),
  };
  let root = composition::CompositionRoot::bootstrap(
    config.clone(),
    composition::RuntimePaths::new(workspace.clone(), home.clone()),
    Arc::new(approval),
  )
  .await?;
  let mut kernel = root.build_kernel()?;

  let result = match command {
    Command::Ask { prompt } => ask(&mut kernel, &prompt).await,
    Command::Chat => chat(&mut kernel).await,
    Command::Resume { session_id } => {
      let id = SessionId(uuid::Uuid::parse_str(&session_id).context("invalid session ID")?);
      let store = SessionStore::open(storage_path(&config, &workspace, &home)).await?;
      let events = store.events(id).await?;
      if events.is_empty() {
        anyhow::bail!("session not found: {id}");
      }
      kernel.resume(&events)?;
      chat(&mut kernel).await
    }
    Command::Sessions => unreachable!("sessions is handled before kernel startup"),
    Command::Tui { .. } => {
      return crate::tui::run(kernel, tui_approvals).await;
    }
  };
  let close = kernel.close_storage().await;
  result?;
  close?;
  Ok(())
}

mod build;
pub mod composition;
use build::storage_path;
mod interactive;
pub use interactive::run_kernel_turn;
use interactive::{ask, chat};

fn load_agent_profile(home: &Path, id: &str) -> Result<AgentProfile> {
  match ProfileLoader::new(home).load(id) {
    Ok(profile) => Ok(profile),
    Err(ProfileError::Io { source, .. })
      if id == "default-dev" && source.kind() == io::ErrorKind::NotFound =>
    {
      Ok(AgentProfile::builtin_default())
    }
    Err(error) => Err(error.into()),
  }
}
