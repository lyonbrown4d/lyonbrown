use crate::{Reply, Scenario};
use anyhow::{Context, Result, ensure};
use clap::Parser;
use config::{Config, File};
use serde::Deserialize;
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(about = "Loopback-only synthetic model server; never calls a real model")]
pub struct Cli {
  #[arg(long, help = "Explicit synthetic scenario TOML or JSON file")]
  pub scenario: Option<PathBuf>,
  #[arg(long)]
  pub listen: Option<SocketAddr>,
  #[arg(long)]
  pub data_dir: Option<PathBuf>,
}
impl Cli {
  pub fn parse_args() -> Self {
    Self::parse()
  }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
  pub listen: SocketAddr,
  pub data_dir: PathBuf,
  #[serde(default = "default_concurrency")]
  pub max_concurrency: usize,
  pub scenarios: Vec<Scenario>,
}
fn default_concurrency() -> usize {
  32
}
impl AppConfig {
  pub fn load(cli: Cli) -> Result<Self> {
    let mut environment = BTreeMap::new();
    let dotenv_path = cli
      .scenario
      .as_ref()
      .and_then(|path| path.parent())
      .unwrap_or_else(|| std::path::Path::new("."))
      .join(".env");
    if dotenv_path.exists() {
      for entry in dotenvy::from_path_iter(dotenv_path)
        .map_err(|_| anyhow::anyhow!("cannot read mock dotenv file"))?
      {
        let (key, value) = entry.map_err(|_| anyhow::anyhow!("invalid mock dotenv entry"))?;
        environment.insert(key, value);
      }
    }
    for (key, value) in std::env::vars_os() {
      let key = key
        .into_string()
        .map_err(|_| anyhow::anyhow!("mock environment must be valid UTF-8"))?;
      let value = value
        .into_string()
        .map_err(|_| anyhow::anyhow!("mock environment must be valid UTF-8"))?;
      environment.insert(key, value);
    }
    let scenario = cli
      .scenario
      .or_else(|| {
        environment
          .get("LYONBROWN_MODEL_MOCK_SCENARIO")
          .map(PathBuf::from)
      })
      .context("--scenario or LYONBROWN_MODEL_MOCK_SCENARIO is required")?;
    ensure!(
      std::fs::metadata(&scenario)
        .context("read synthetic scenario metadata")?
        .len()
        <= 4 * 1024 * 1024,
      "scenario file exceeds 4 MiB"
    );
    let data_dir = directories::ProjectDirs::from("com", "lyonbrown", "lyonbrown")
      .context("user data directory is unavailable")?
      .data_local_dir()
      .join("model-mock");
    let mut builder = Config::builder()
      .set_default("listen", "127.0.0.1:8100")?
      .set_default(
        "data_dir",
        data_dir.to_str().context("data directory must be UTF-8")?,
      )?
      .add_source(File::from(scenario));
    for (key, field) in [
      ("LYONBROWN_MODEL_MOCK_LISTEN", "listen"),
      ("LYONBROWN_MODEL_MOCK_DATA_DIR", "data_dir"),
    ] {
      if let Some(value) = environment.get(key) {
        builder = builder.set_override(field, value.clone())?;
      }
    }
    let mut settings: Self = builder
      .build()
      .and_then(Config::try_deserialize)
      .map_err(|_| anyhow::anyhow!("invalid synthetic scenario configuration"))?;
    if let Some(listen) = cli.listen {
      settings.listen = listen;
    }
    if let Some(data_dir) = cli.data_dir {
      settings.data_dir = data_dir;
    }
    settings.validate()?;
    Ok(settings)
  }
  pub fn validate(&self) -> Result<()> {
    ensure!(
      self.listen.ip().is_loopback(),
      "model mock listener must be loopback"
    );
    ensure!(
      self.data_dir.is_absolute(),
      "model mock data directory must be absolute"
    );
    ensure!(
      (1..=256).contains(&self.max_concurrency),
      "mock concurrency must be 1..=256"
    );
    ensure!(
      (1..=128).contains(&self.scenarios.len()),
      "configure 1..=128 synthetic scenarios"
    );
    ensure!(
      self
        .scenarios
        .iter()
        .map(|scenario| scenario.replies.len())
        .sum::<usize>()
        <= 1024,
      "configure at most 1024 total replies"
    );
    for (index, scenario) in self.scenarios.iter().enumerate() {
      ensure!(
        !scenario.model.is_empty()
          && scenario.model.len() <= 128
          && scenario
            .model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_/.:".contains(&byte)),
        "invalid synthetic model id"
      );
      ensure!(
        !self.scenarios[..index]
          .iter()
          .any(|other| other.model == scenario.model && other.protocol == scenario.protocol),
        "duplicate model/protocol scenario"
      );
      ensure!(
        (1..=128).contains(&scenario.replies.len()),
        "scenario requires 1..=128 replies"
      );
      for reply in &scenario.replies {
        ensure!(
          !(scenario.protocol == crate::Protocol::OllamaGenerate
            && matches!(reply, Reply::Tool { .. })),
          "Ollama generate does not support tools"
        );
        ensure!(reply.delay_ms() <= 60_000, "mock delay exceeds 60 seconds");
        match reply {
          Reply::Text { text, .. }
          | Reply::Decision { text, .. }
          | Reply::Truncated { text }
          | Reply::MissingTerminal { text }
          | Reply::Malformed { body: text } => {
            ensure!(text.len() <= 65_536, "mock text exceeds 64 KiB")
          }
          Reply::Tool {
            name, arguments, ..
          } => ensure!(
            !name.is_empty() && name.len() <= 128 && arguments.to_string().len() <= 65_536,
            "invalid synthetic tool fixture"
          ),
          Reply::Error { status, .. } => ensure!(
            (400..=599).contains(status),
            "mock error status must be 400..=599"
          ),
          Reply::Stall => {}
        }
      }
    }
    Ok(())
  }
}
