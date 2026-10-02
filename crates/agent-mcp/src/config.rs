use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
  pub command: PathBuf,
  #[serde(default)]
  pub args: Vec<String>,
}
