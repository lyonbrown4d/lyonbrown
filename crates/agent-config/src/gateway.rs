//! Host configuration. Credentials are references resolved from layered secrets.
use std::{collections::BTreeSet, net::SocketAddr};

use anyhow::{Result, ensure};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GatewaySettings {
  /// None keeps the embedded service private; no socket is opened.
  pub http_listen: Option<SocketAddr>,
  pub http_token_env: Option<String>,
  pub models: Vec<GatewayModelSettings>,
  pub endpoints: Vec<GatewayEndpointSettings>,
  pub groups: Vec<GatewayGroupSettings>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayModelSettings {
  pub id: String,
  pub upstream_model: String,
  pub api_base: String,
  pub api_key_env: Option<String>,
  pub context_tokens: u64,
  #[serde(default)]
  pub capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayEndpointSettings {
  pub id: String,
  pub provider_id: String,
  pub api_base: String,
  pub api_key_env: Option<String>,
  pub model_ids: BTreeSet<String>,
  #[serde(default)]
  pub priority: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayGroupSettings {
  pub id: String,
  pub models: Vec<String>,
}

impl GatewaySettings {
  pub fn credential_references(&self) -> impl Iterator<Item = &str> {
    self
      .http_token_env
      .iter()
      .chain(
        self
          .models
          .iter()
          .filter_map(|model| model.api_key_env.as_ref()),
      )
      .chain(
        self
          .endpoints
          .iter()
          .filter_map(|endpoint| endpoint.api_key_env.as_ref()),
      )
      .map(String::as_str)
  }

  pub fn validate(&self) -> Result<()> {
    if let Some(listen) = self.http_listen {
      ensure!(
        listen.ip().is_loopback() && listen.port() != 0,
        "gateway HTTP requires a nonzero loopback socket; remote TLS is not configured"
      );
      ensure!(
        self.http_token_env.is_some(),
        "gateway HTTP requires a dedicated token reference"
      );
    }
    for name in self.credential_references() {
      ensure!(
        !name.is_empty()
          && name.bytes().enumerate().all(|(index, byte)| byte == b'_'
            || byte.is_ascii_alphabetic()
            || index > 0 && byte.is_ascii_digit()),
        "invalid gateway credential reference"
      );
    }
    ensure!(
      self.models.len() <= 256 && self.endpoints.len() <= 1024 && self.groups.len() <= 256,
      "gateway configuration exceeds capacity"
    );
    let ids: BTreeSet<_> = self.models.iter().map(|model| model.id.as_str()).collect();
    ensure!(ids.len() == self.models.len(), "duplicate gateway model");
    for model in &self.models {
      ensure!(
        !model.id.trim().is_empty()
          && !model.upstream_model.trim().is_empty()
          && model.context_tokens > 0,
        "invalid gateway model"
      );
      crate::settings::validate_url(&model.api_base, "gateway upstream")?;
    }
    let mut groups = BTreeSet::new();
    for group in &self.groups {
      ensure!(
        !group.id.trim().is_empty()
          && !ids.contains(group.id.as_str())
          && groups.insert(&group.id)
          && !group.models.is_empty()
          && group
            .models
            .iter()
            .all(|model| ids.contains(model.as_str())),
        "invalid gateway model group"
      );
    }
    for endpoint in &self.endpoints {
      ensure!(
        !endpoint.model_ids.is_empty()
          && endpoint
            .model_ids
            .iter()
            .all(|model| ids.contains(model.as_str())),
        "gateway endpoint references an unknown model"
      );
      crate::settings::validate_url(&endpoint.api_base, "gateway upstream")?;
    }
    Ok(())
  }
}
