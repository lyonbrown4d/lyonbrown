use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "agentd", version, about = "Local Lyonbrown agent daemon")]
pub struct Args {
  /// Keep the daemon attached to the launching process.
  #[arg(long)]
  pub foreground: bool,

  /// Workspace served by this daemon instance.
  #[arg(long, default_value = ".")]
  pub workspace: PathBuf,

  /// Explicit runtime configuration file.
  #[arg(long)]
  pub config: Option<PathBuf>,

  /// Fingerprint of the effective client runtime configuration.
  #[arg(long, value_parser = non_blank)]
  pub runtime_config_fingerprint: String,

  /// Model provider selected for this runtime.
  #[arg(long)]
  pub provider: Option<String>,

  /// Agent profile ID loaded by the daemon runtime.
  #[arg(long)]
  pub profile: Option<String>,

  /// Provider-specific model ID.
  #[arg(long)]
  pub model: Option<String>,

  /// Ollama base URL.
  #[arg(long)]
  pub endpoint: Option<String>,

  /// Allow every tool request for this daemon session.
  #[arg(long, conflicts_with = "deny_all")]
  pub approve_all: bool,

  /// Deny every write, execute, and network tool request.
  #[arg(long, conflicts_with = "approve_all")]
  pub deny_all: bool,

  /// Trust MCP commands declared by the workspace configuration.
  #[arg(long)]
  pub trust_workspace_mcp: bool,

  /// Trust model, approval, and storage workspace settings.
  #[arg(long)]
  pub trust_workspace_config: bool,
}

fn non_blank(value: &str) -> Result<String, String> {
  if value.trim().is_empty() {
    Err("value must not be blank".to_owned())
  } else {
    Ok(value.to_owned())
  }
}
