use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Deserialize;

/// Explicit outbound routes. These settings never create peer trust.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelaySettings {
  pub control_node_id: String,
  pub endpoint: String,
  pub routes: BTreeSet<String>,
}

impl RelaySettings {
  pub fn validate(&self) -> Result<()> {
    let identifier = |value: &str| {
      !value.is_empty()
        && value.len() <= 128
        && value
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
    };
    ensure!(
      identifier(&self.control_node_id),
      "invalid relay control identity"
    );
    ensure!(
      (1..=32).contains(&self.routes.len())
        && self
          .routes
          .iter()
          .all(|id| identifier(id) && id != &self.control_node_id),
      "relay requires 1..32 distinct peer routes excluding its control identity"
    );
    let url = url::Url::parse(&self.endpoint)
      .map_err(|_| anyhow::anyhow!("invalid relay control origin"))?;
    ensure!(
      self.endpoint.len() <= 2048
        && url.scheme() == "https"
        && url.host_str().is_some_and(|host| !host.contains('*'))
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/",
      "relay control must be a credential-free HTTPS origin"
    );
    Ok(())
  }
}
