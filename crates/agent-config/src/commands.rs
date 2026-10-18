use std::{
  collections::{BTreeMap, BTreeSet},
  path::PathBuf,
};

use anyhow::{Result, ensure};
use serde::Deserialize;

/// Trusted operator policy. Empty executables disable command tools.
/// Environment values name dotenv/environment variables, never literal secrets.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CommandSettings {
  pub executables: BTreeSet<PathBuf>,
  pub environment: BTreeMap<String, String>,
}

impl CommandSettings {
  pub fn validate(&self) -> Result<()> {
    ensure!(
      self.executables.len() <= 64,
      "at most 64 command executables are permitted"
    );
    ensure!(
      self.executables.iter().all(|path| path.is_absolute()),
      "command executables must be absolute"
    );
    ensure!(
      self.environment.len() <= 64
        && self
          .environment
          .iter()
          .all(|(key, reference)| valid_name(key) && valid_name(reference)),
      "invalid command environment key or reference"
    );
    Ok(())
  }
}

fn valid_name(value: &str) -> bool {
  let mut bytes = value.bytes();
  value.len() <= 128
    && bytes
      .next()
      .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
    && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
