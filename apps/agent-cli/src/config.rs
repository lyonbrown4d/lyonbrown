use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ThemeChoice {
  #[default]
  Auto,
  Light,
  Dark,
}

/// Authenticated terminal client for the Lyonbrown agent daemon.
#[derive(Parser)]
#[command(version, about)]
pub struct Config {
  /// Daemon HTTP/2 endpoint.
  #[arg(
    long,
    env = "LYONBROWN_ENDPOINT",
    default_value = "http://127.0.0.1:7447"
  )]
  pub endpoint: String,
  /// Remote daemon bearer token. Prefer the environment variable to shell history.
  #[arg(long, env = "LYONBROWN_TOKEN", hide_env_values = true)]
  pub token: Option<String>,
  /// Local application data directory (also used to find the daemon token).
  #[arg(long, env = "LYONBROWN_DATA_DIR")]
  pub data_dir: Option<PathBuf>,
  /// Configuration TOML file; .env is read from its directory.
  #[arg(long, env = "LYONBROWN_CONFIG")]
  pub config: Option<PathBuf>,
  #[arg(long, value_enum, default_value = "auto")]
  pub theme: ThemeChoice,
  #[command(subcommand)]
  pub command: Option<crate::task_command::Command>,
}

impl Config {
  pub fn resolve(self, endpoint_overridden: bool) -> anyhow::Result<ResolvedConfig> {
    let data_dir = match self.data_dir {
      Some(path) => path,
      None => agent_config::default_data_dir()?,
    };
    let data_dir = if data_dir.is_absolute() {
      data_dir
    } else {
      std::env::current_dir()?.join(data_dir)
    };
    let mut settings = agent_config::load(&data_dir, self.config.as_deref())?;
    if endpoint_overridden {
      settings.endpoint = self.endpoint;
    }
    settings.validate()?;
    Ok(ResolvedConfig {
      endpoint: settings.endpoint.clone(),
      token: self.token.or_else(|| settings.credential("daemon")),
      data_dir,
      theme: self.theme,
      command: self.command,
      settings,
    })
  }
}

pub struct ResolvedConfig {
  pub endpoint: String,
  pub token: Option<String>,
  pub data_dir: PathBuf,
  pub theme: ThemeChoice,
  pub settings: agent_config::Settings,
  pub command: Option<crate::task_command::Command>,
}
