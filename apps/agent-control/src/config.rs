use std::{
  collections::BTreeMap,
  net::SocketAddr,
  path::{Path, PathBuf},
};

use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use serde::Deserialize;

#[derive(Parser)]
#[command(about = "Self-hosted Lyonbrown identity directory and opaque outbound relay")]
pub struct Arguments {
  #[arg(long)]
  pub data_dir: Option<PathBuf>,
  #[arg(long)]
  pub config: Option<PathBuf>,
  #[arg(long)]
  pub listen: Option<SocketAddr>,
  #[command(subcommand)]
  pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
  Serve,
  /// Export only the public TLS/Noise identity for out-of-band human verification.
  Identity,
  /// Import an operator-verified public identity. Restart to reload mTLS roots.
  Pair {
    bundle: PathBuf,
    #[arg(long)]
    pairing_fingerprint: String,
  },
  Revoke {
    node_id: String,
  },
  /// Authorize one direction only; authorize the reverse separately if required.
  Allow {
    source: String,
    target: String,
  },
  Deny {
    source: String,
    target: String,
  },
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
  pub node_id: String,
  pub listen: SocketAddr,
  pub lease_seconds: u64,
  pub queue_capacity: usize,
  pub log_directory: PathBuf,
  pub log_retention: usize,
  pub log_filter: String,
  /// When present, this file's graph replaces the persisted operator graph atomically.
  pub allowed_links: Option<Vec<AllowedLink>>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedLink {
  pub source: String,
  pub target: String,
}

pub fn load(data_dir: &Path, arguments: &Arguments) -> Result<Settings> {
  let path = arguments
    .config
    .clone()
    .unwrap_or_else(|| data_dir.join("control.toml"));
  let dotenv_path = path.parent().unwrap_or(data_dir).join(".env");
  let mut environment = BTreeMap::new();
  if dotenv_path.try_exists()? {
    for entry in dotenvy::from_path_iter(&dotenv_path)? {
      let (key, value) = entry.map_err(|_| anyhow::anyhow!("invalid control dotenv entry"))?;
      environment.insert(key, value);
    }
  }
  environment.extend(
    std::env::vars_os()
      .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))),
  );
  let mut builder = config::Config::builder()
    .set_default("node_id", "control")?
    .set_default("listen", "127.0.0.1:7449")?
    .set_default("lease_seconds", 60)?
    .set_default("queue_capacity", 32)?
    .set_default(
      "log_directory",
      data_dir.join("logs").to_string_lossy().into_owned(),
    )?
    .set_default("log_retention", 14)?
    .set_default("log_filter", "info")?
    .add_source(config::File::from(path.as_path()).required(arguments.config.is_some()));
  for (key, field) in [
    ("LYONBROWN_CONTROL_NODE_ID", "node_id"),
    ("LYONBROWN_CONTROL_LISTEN", "listen"),
    ("LYONBROWN_CONTROL_LEASE_SECONDS", "lease_seconds"),
    ("LYONBROWN_CONTROL_QUEUE_CAPACITY", "queue_capacity"),
    ("LYONBROWN_CONTROL_LOG_FILTER", "log_filter"),
    ("LYONBROWN_CONTROL_LOG_DIRECTORY", "log_directory"),
    ("LYONBROWN_CONTROL_LOG_RETENTION", "log_retention"),
  ] {
    if let Some(value) = environment.get(key) {
      builder = builder.set_override(field, value.clone())?;
    }
  }
  let mut settings: Settings = builder
    .build()
    .and_then(config::Config::try_deserialize)
    .map_err(|_| anyhow::anyhow!("invalid control configuration"))?;
  if let Some(listen) = arguments.listen {
    settings.listen = listen;
  }
  ensure!(
    !settings.node_id.is_empty() && settings.node_id.len() <= 128,
    "invalid control node identity"
  );
  ensure!(
    settings.listen.port() != 0,
    "control listen port must be nonzero"
  );
  ensure!(
    (1..=3600).contains(&settings.lease_seconds),
    "invalid directory lease"
  );
  ensure!(
    (1..=1024).contains(&settings.queue_capacity),
    "invalid relay queue capacity"
  );
  ensure!(
    (1..=365).contains(&settings.log_retention),
    "invalid log retention"
  );
  if settings.log_directory.is_relative() {
    settings.log_directory = data_dir.join(&settings.log_directory);
  }
  Ok(settings)
}
