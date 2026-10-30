use std::{collections::BTreeSet, fmt};

use anyhow::{Result, ensure};
use decision_core::{Constraints, ModelTier};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use url::Url;

/// Credentials deliberately have no serialization implementation.
#[derive(Clone, Debug, Deserialize)]
#[serde(transparent)]
pub struct Secret(SecretString);
impl Secret {
  pub fn new(value: impl Into<String>) -> Self {
    Self(value.into().into())
  }
  /// Explicit access for trusted composition and credential transport.
  pub fn expose_secret(&self) -> &str {
    self.0.expose_secret()
  }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
  pub id: String,
  pub upstream_model: String,
  pub api_base: String,
  pub api_key: Option<Secret>,
  /// Application loaders resolve this named credential from dotenv/environment.
  pub api_key_env: Option<String>,
  pub tier: ModelTier,
  /// Trusted operator assertion that this endpoint runs local inference, not a cloud proxy.
  #[serde(default)]
  pub local_inference: bool,
  #[serde(default)]
  pub capabilities: BTreeSet<String>,
  pub context_tokens: Option<u64>,
}
impl fmt::Debug for ModelConfig {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ModelConfig")
      .field("id", &self.id)
      .field("upstream_model", &self.upstream_model)
      .field("api_base", &"[REDACTED]")
      .field("api_key", &self.api_key)
      .field("api_key_env", &self.api_key_env)
      .field("tier", &self.tier)
      .field("local_inference", &self.local_inference)
      .field("capabilities", &self.capabilities)
      .field("context_tokens", &self.context_tokens)
      .finish()
  }
}
impl ModelConfig {
  pub fn validate(&self) -> Result<()> {
    ensure!(
      !self.id.is_empty() && !matches!(self.id.as_str(), "router/auto" | "auto"),
      "invalid model id"
    );
    ensure!(
      !self.upstream_model.trim().is_empty(),
      "empty upstream model"
    );
    let url =
      Url::parse(&self.api_base).map_err(|_| anyhow::anyhow!("invalid upstream API base"))?;
    ensure!(
      matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
      "invalid upstream scheme or host"
    );
    ensure!(
      url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none(),
      "upstream URL must not contain credentials, query or fragment"
    );
    let loopback = url
      .host_str()
      .and_then(|host| {
        host
          .trim_matches(['[', ']'])
          .parse::<std::net::IpAddr>()
          .ok()
      })
      .is_some_and(|ip| ip.is_loopback());
    ensure!(
      url.scheme() == "https" || loopback,
      "remote upstream requires HTTPS"
    );
    ensure!(
      !self.local_inference || loopback,
      "trusted local inference requires a literal loopback address"
    );
    ensure!(
      self.context_tokens.is_some_and(|n| n > 0),
      "model context capacity is required"
    );
    ensure!(
      self
        .api_key
        .as_ref()
        .is_none_or(|key| !key.expose_secret().is_empty()),
      "empty upstream credential"
    );
    Ok(())
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
  pub max_body_bytes: usize,
  pub max_response_bytes: usize,
  pub max_concurrent: usize,
  pub first_byte_ms: u64,
  pub total_ms: u64,
  pub default_output_tokens: u64,
}
impl Default for Limits {
  fn default() -> Self {
    Self {
      max_body_bytes: 262_144,
      max_response_bytes: 16_777_216,
      max_concurrent: 8,
      first_byte_ms: 30_000,
      total_ms: 180_000,
      default_output_tokens: 1024,
    }
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
  pub token: Secret,
  pub models: Vec<ModelConfig>,
  #[serde(default)]
  pub endpoints: Vec<crate::ProviderEndpoint>,
  #[serde(default)]
  pub limits: Limits,
  #[serde(default)]
  pub constraints: Constraints,
}
impl GatewayConfig {
  pub fn validate(&self) -> Result<()> {
    ensure!(
      (32..=512).contains(&self.token.expose_secret().len())
        && self
          .token
          .expose_secret()
          .bytes()
          .all(|b| b.is_ascii_graphic()),
      "gateway token requires 32..=512 printable ASCII bytes"
    );
    let limits = &self.limits;
    ensure!(
      (1..=128).contains(&limits.max_concurrent)
        && (1024..=16_777_216).contains(&limits.max_body_bytes),
      "invalid gateway capacity"
    );
    ensure!(
      (1024..=67_108_864).contains(&limits.max_response_bytes),
      "invalid response capacity"
    );
    ensure!(
      limits.first_byte_ms > 0
        && limits.first_byte_ms <= limits.total_ms
        && limits.total_ms <= 3_600_000
        && limits.default_output_tokens > 0,
      "invalid gateway deadline or output budget"
    );
    ensure!(
      !self.models.is_empty() && self.models.len() <= 256,
      "gateway requires 1..=256 configured models"
    );
    let mut ids = BTreeSet::new();
    for model in &self.models {
      model.validate()?;
      ensure!(ids.insert(&model.id), "duplicate model id");
    }
    ensure!(self.endpoints.len() <= 1024, "too many provider endpoints");
    let mut endpoints = BTreeSet::new();
    for endpoint in &self.endpoints {
      endpoint.validate()?;
      ensure!(
        endpoints.insert(&endpoint.id),
        "duplicate provider endpoint id"
      );
      ensure!(
        !endpoint.model_ids.is_empty() && endpoint.model_ids.iter().all(|id| ids.contains(id)),
        "provider endpoint references an unknown model"
      );
    }
    Ok(())
  }
}

/// Public registry metadata never includes endpoints or credentials.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct PublicModel {
  pub id: String,
  pub object: &'static str,
  pub created: u64,
  pub owned_by: &'static str,
}
