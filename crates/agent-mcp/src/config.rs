use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
  pub command: PathBuf,
  #[serde(default)]
  pub args: Vec<String>,
}
