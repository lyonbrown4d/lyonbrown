use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, ensure};
use config::{Config, File};

use crate::settings::{Credentials, Settings};

const ALIASES: &[(&str, &str)] = &[
  ("LYONBROWN_LISTEN", "listen"),
  ("LYONBROWN_ENDPOINT", "endpoint"),
  ("LYONBROWN_PROVIDER", "provider"),
  ("LYONBROWN_MODEL", "model"),
  ("LYONBROWN_SUPPORTS_IMAGES", "supports_images"),
  ("LYONBROWN_BROWSER_TOOLS", "browser_tools"),
  ("LYONBROWN_COMPUTER_TOOLS", "computer_tools"),
  ("LYONBROWN_COMPUTER_LEASE_SECS", "computer_lease_secs"),
  ("LYONBROWN_API_BASE", "api_base"),
  ("LYONBROWN_MAX_RUNS", "max_runs"),
  ("LYONBROWN_RUN_TIMEOUT_SECS", "run_timeout_secs"),
  ("LYONBROWN_LOG_DIRECTORY", "log_directory"),
  ("LYONBROWN_LOG_RETENTION", "log_retention"),
  ("LYONBROWN_PEER_LISTEN", "peer.listen"),
  ("LYONBROWN_PEER_ADVERTISE_URL", "peer.advertise_url"),
  ("LYONBROWN_PEER_MDNS", "peer.mdns"),
  ("LYONBROWN_DECISION_MODEL", "decision.model"),
  ("LYONBROWN_DECISION_PROVIDER", "decision.provider"),
  ("LYONBROWN_DECISION_API_BASE", "decision.api_base"),
  ("LYONBROWN_DECISION_TIMEOUT_MS", "decision.timeout_ms"),
  (
    "LYONBROWN_DECISION_MINIMUM_CONFIDENCE",
    "decision.minimum_confidence",
  ),
];

pub fn load(data_dir: &Path, config_path: Option<&Path>) -> Result<Settings> {
  let environment = std::env::vars_os()
    .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
    .collect();
  load_with_environment(data_dir, config_path, &environment)
}

/// An explicit environment snapshot keeps configuration deterministic in tests and embedding.
pub fn load_with_environment(
  data_dir: &Path,
  config_path: Option<&Path>,
  environment: &BTreeMap<String, String>,
) -> Result<Settings> {
  ensure!(
    data_dir.is_absolute(),
    "configuration data directory must be absolute"
  );
  let default_path = data_dir.join("config.toml");
  let path = config_path.unwrap_or(&default_path);
  let dotenv_path = path.parent().unwrap_or(data_dir).join(".env");
  let mut merged = read_dotenv(&dotenv_path)?;
  merged.extend(
    environment
      .iter()
      .map(|(key, value)| (key.clone(), value.clone())),
  );
  let mut builder = Config::builder()
    .set_default("listen", "127.0.0.1:7447")?
    .set_default("endpoint", "http://127.0.0.1:7447")?
    .set_default("provider", "openai")?
    .set_default("model", "gpt-4.1")?
    .set_default("max_runs", 4)?
    .set_default("run_timeout_secs", 900)?
    .set_default(
      "log_directory",
      data_dir
        .join("logs")
        .to_str()
        .context("log directory must be UTF-8")?,
    )?
    .set_default("log_retention", 14)?
    .add_source(File::from(path).required(config_path.is_some()));
  for &(environment_key, field) in ALIASES {
    if let Some(value) = merged.get(environment_key) {
      builder = builder.set_override(field, value.clone())?;
    }
  }
  // Configuration parser errors may contain excerpts; do not expose file content.
  let config = builder
    .build()
    .map_err(|_| anyhow::anyhow!("unable to parse configuration file {}", path.display()))?;
  let mut settings: Settings = config
    .try_deserialize()
    .map_err(|_| anyhow::anyhow!("configuration has unknown fields or invalid value types"))?;
  if settings.log_directory.is_relative() {
    settings.log_directory = data_dir.join(&settings.log_directory);
  }
  settings.credentials = Credentials(
    [
      ("openai", "OPENAI_API_KEY"),
      ("anthropic", "ANTHROPIC_API_KEY"),
      ("daemon", "LYONBROWN_TOKEN"),
    ]
    .into_iter()
    .filter_map(|(provider, key)| {
      merged
        .get(key)
        .map(|value| (provider.into(), value.clone()))
    })
    .collect(),
  );
  settings.validate()?;
  for server in &settings.mcp {
    for name in server.environment_references() {
      let value = merged
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("MCP environment reference {name} is missing"))?;
      ensure!(
        !value.contains('\0'),
        "MCP environment value contains an invalid byte"
      );
      settings
        .credentials
        .0
        .insert(format!("mcp:{name}"), value.clone());
    }
  }
  for name in settings.commands.environment.values() {
    let value = merged
      .get(name)
      .ok_or_else(|| anyhow::anyhow!("command environment reference {name} is missing"))?;
    ensure!(
      !value.contains('\0'),
      "command environment value contains an invalid byte"
    );
    settings
      .credentials
      .0
      .insert(format!("command:{name}"), value.clone());
  }
  Ok(settings)
}

fn read_dotenv(path: &Path) -> Result<BTreeMap<String, String>> {
  if !path.try_exists()? {
    return Ok(BTreeMap::new());
  }
  let iterator = dotenvy::from_path_iter(path)
    .map_err(|_| anyhow::anyhow!("unable to read dotenv file {}", path.display()))?;
  iterator
    .map(|entry| entry.map_err(|_| anyhow::anyhow!("invalid dotenv entry in {}", path.display())))
    .collect()
}
