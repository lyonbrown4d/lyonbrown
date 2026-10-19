use std::{collections::BTreeMap, fmt, net::SocketAddr, path::PathBuf};

use anyhow::{Result, ensure};
use serde::Deserialize;
use url::Url;

#[derive(Clone, Default)]
pub(crate) struct Credentials(pub BTreeMap<String, String>);

impl fmt::Debug for Credentials {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("Credentials([REDACTED])")
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
  pub listen: SocketAddr,
  pub endpoint: String,
  pub provider: String,
  pub model: String,
  #[serde(default)]
  pub local_models: Vec<crate::LocalModelSettings>,
  #[serde(default)]
  pub mcp: Vec<crate::McpSettings>,
  #[serde(default)]
  pub commands: crate::CommandSettings,
  #[serde(default)]
  pub delegation_targets: Vec<crate::DelegationTargetSettings>,
  #[serde(default = "enabled")]
  pub browser_tools: bool,
  #[serde(default)]
  pub computer_tools: bool,
  #[serde(default = "computer_lease_secs")]
  pub computer_lease_secs: u64,
  #[serde(default)]
  pub supports_images: bool,
  #[serde(default)]
  pub api_base: Option<String>,
  pub max_runs: usize,
  pub run_timeout_secs: u64,
  pub log_directory: PathBuf,
  pub log_retention: usize,
  #[serde(default)]
  pub decision: DecisionSettings,
  #[serde(default)]
  pub peer: Option<crate::PeerSettings>,
  #[serde(default)]
  pub relay: Option<crate::RelaySettings>,
  #[serde(skip)]
  pub(crate) credentials: Credentials,
}

fn computer_lease_secs() -> u64 {
  300
}

fn enabled() -> bool {
  true
}

impl Settings {
  pub fn command_environment_value(&self, name: &str) -> Option<&str> {
    self
      .credentials
      .0
      .get(&format!("command:{name}"))
      .map(String::as_str)
  }
  pub fn mcp_environment_value(&self, name: &str) -> Option<&str> {
    self
      .credentials
      .0
      .get(&format!("mcp:{name}"))
      .map(String::as_str)
  }
  pub fn credential(&self, provider: &str) -> Option<String> {
    self
      .credentials
      .0
      .get(provider)
      .filter(|value| !value.trim().is_empty())
      .cloned()
  }

  /// Revalidate after applying explicit command-line overrides.
  pub fn validate(&self) -> Result<()> {
    ensure!(
      (10..=300).contains(&self.computer_lease_secs),
      "computer lease must be within 10..=300 seconds"
    );
    ensure!(
      self.listen.ip().is_loopback() && self.listen.port() != 0,
      "local daemon listen address must be a nonzero loopback socket"
    );
    validate_url(&self.endpoint, "endpoint")?;
    if let Some(base) = &self.api_base {
      validate_url(base, "api_base")?;
    }
    ensure!(
      !self.provider.trim().is_empty(),
      "provider must not be empty"
    );
    ensure!(!self.model.trim().is_empty(), "model must not be empty");
    ensure!(
      (1..=128).contains(&self.max_runs),
      "max_runs must be between 1 and 128"
    );
    ensure!(
      (1..=86_400).contains(&self.run_timeout_secs),
      "run_timeout_secs must be between 1 and 86400"
    );
    ensure!(
      (1..=365).contains(&self.log_retention),
      "log_retention must be between 1 and 365 files"
    );
    ensure!(
      self.log_directory.is_absolute(),
      "log_directory must resolve to an absolute path"
    );
    crate::validate_local_models(&self.local_models)?;
    crate::validate_mcp(&self.mcp)?;
    self.commands.validate()?;
    crate::validate_delegation_targets(&self.delegation_targets)?;
    self.decision.validate()?;
    if let Some(peer) = &self.peer {
      peer.validate()?;
    }
    if let Some(relay) = &self.relay {
      relay.validate()?;
      ensure!(
        self.peer.is_some(),
        "relay requires the peer runtime to be configured"
      );
    }
    Ok(())
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DecisionSettings {
  pub policy: Option<decision_core::Policy>,
  /// None selects the explicit workspace rule without a classifier request.
  pub model: Option<String>,
  pub provider: Option<String>,
  pub api_base: Option<String>,
  pub timeout_ms: u64,
  pub minimum_confidence: f64,
}
impl Default for DecisionSettings {
  fn default() -> Self {
    Self {
      policy: None,
      model: None,
      provider: None,
      api_base: None,
      timeout_ms: 3000,
      minimum_confidence: 0.75,
    }
  }
}
impl DecisionSettings {
  pub fn validate(&self) -> Result<()> {
    if let Some(policy) = &self.policy {
      decision_core::PolicySnapshot::new(policy.clone())?;
    }
    ensure!(
      (1..=60_000).contains(&self.timeout_ms),
      "decision timeout_ms must be between 1 and 60000"
    );
    ensure!(
      self.minimum_confidence.is_finite() && (0.0..=1.0).contains(&self.minimum_confidence),
      "decision minimum_confidence must be between 0 and 1"
    );
    ensure!(
      self
        .model
        .as_ref()
        .is_none_or(|value| !value.trim().is_empty()),
      "decision model must not be empty"
    );
    ensure!(
      self
        .provider
        .as_ref()
        .is_none_or(|value| !value.trim().is_empty()),
      "decision provider must not be empty"
    );
    if let Some(base) = &self.api_base {
      validate_url(base, "decision api_base")?;
    }
    Ok(())
  }
}

fn validate_url(value: &str, field: &str) -> Result<()> {
  // Do not include the supplied URL in errors: credentials and query tokens may be present.
  let url =
    Url::parse(value).map_err(|_| anyhow::anyhow!("{field} must be an absolute HTTP(S) URL"))?;
  ensure!(
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
    "{field} must be an absolute HTTP(S) URL"
  );
  ensure!(
    url.username().is_empty()
      && url.password().is_none()
      && url.query().is_none()
      && url.fragment().is_none(),
    "{field} must not contain URL credentials, a query, or a fragment"
  );
  Ok(())
}
