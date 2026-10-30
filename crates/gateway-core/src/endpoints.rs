use std::{
  collections::{BTreeSet, HashSet},
  fmt,
  net::IpAddr,
  sync::{Arc, Mutex},
  time::Duration,
};

use anyhow::{Result, ensure};
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;

use crate::Secret;

/// Upper bound applied to provider cooldowns requested by transport callers.
pub const MAX_ENDPOINT_COOLDOWN: Duration = Duration::from_secs(300);

/// A provider endpoint may serve only the explicitly listed gateway model IDs.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEndpoint {
  pub id: String,
  pub provider_id: String,
  pub api_base: String,
  pub api_key: Option<Secret>,
  pub api_key_env: Option<String>,
  #[serde(default = "default_enabled")]
  pub enabled: bool,
  #[serde(default)]
  pub priority: u32,
  #[serde(default = "default_endpoint_concurrency")]
  pub max_concurrent: usize,
  #[serde(default)]
  pub model_ids: BTreeSet<String>,
}

fn default_enabled() -> bool {
  true
}

fn default_endpoint_concurrency() -> usize {
  1
}

impl fmt::Debug for ProviderEndpoint {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ProviderEndpoint")
      .field("id", &self.id)
      .field("provider_id", &self.provider_id)
      .field("api_base", &"[REDACTED]")
      .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
      .field("api_key_env", &self.api_key_env)
      .field("enabled", &self.enabled)
      .field("priority", &self.priority)
      .field("max_concurrent", &self.max_concurrent)
      .field("model_ids", &self.model_ids)
      .finish()
  }
}

impl ProviderEndpoint {
  /// Validates endpoint identity, URL transport safety, credentials, and capacity.
  pub fn validate(&self) -> Result<()> {
    ensure!(valid_identifier(&self.id), "invalid provider endpoint id");
    ensure!(valid_identifier(&self.provider_id), "invalid provider id");
    ensure!(
      self.max_concurrent > 0 && self.max_concurrent <= Semaphore::MAX_PERMITS,
      "invalid provider endpoint capacity"
    );

    let url = Url::parse(&self.api_base)
      .map_err(|_| anyhow::anyhow!("invalid provider endpoint API base"))?;
    ensure!(
      matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
      "invalid provider endpoint scheme or host"
    );
    ensure!(
      url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none(),
      "provider endpoint URL must not contain credentials, query or fragment"
    );
    ensure!(
      url.scheme() == "https" || is_literal_loopback(&url),
      "remote provider endpoint requires HTTPS"
    );
    ensure!(
      self
        .api_key
        .as_ref()
        .is_none_or(|key| !key.expose_secret().is_empty()),
      "empty provider endpoint credential"
    );
    ensure!(
      self
        .api_key_env
        .as_ref()
        .is_none_or(|name| valid_env_name(name)),
      "invalid provider endpoint credential environment variable"
    );
    ensure!(
      self.model_ids.iter().all(|id| !id.is_empty()),
      "invalid provider endpoint model id"
    );
    Ok(())
  }
}

fn valid_identifier(value: &str) -> bool {
  !value.is_empty()
    && value
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
}

fn valid_env_name(value: &str) -> bool {
  let mut bytes = value.bytes();
  bytes
    .next()
    .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
    && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_literal_loopback(url: &Url) -> bool {
  url
    .host_str()
    .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
    .is_some_and(|ip| ip.is_loopback())
}

struct EndpointState {
  config: ProviderEndpoint,
  permits: Arc<Semaphore>,
  cooldown_until: Mutex<Option<tokio::time::Instant>>,
}

/// Immutable endpoint configuration with process-local admission and cooldown state.
pub struct EndpointRegistry {
  endpoints: Vec<Arc<EndpointState>>,
}

impl EndpointRegistry {
  pub fn new(endpoints: impl IntoIterator<Item = ProviderEndpoint>) -> Result<Self> {
    let mut seen = HashSet::new();
    let mut states = Vec::new();
    for endpoint in endpoints {
      endpoint.validate()?;
      ensure!(
        seen.insert(endpoint.id.clone()),
        "duplicate provider endpoint id"
      );
      states.push(Arc::new(EndpointState {
        permits: Arc::new(Semaphore::new(endpoint.max_concurrent)),
        config: endpoint,
        cooldown_until: Mutex::new(None),
      }));
    }
    states.sort_by(|left, right| {
      left
        .config
        .priority
        .cmp(&right.config.priority)
        .then_with(|| left.config.id.cmp(&right.config.id))
    });
    Ok(Self { endpoints: states })
  }

  /// Selects one endpoint exactly once and reserves capacity without waiting.
  /// A local-only selection admits literal loopback addresses only; it does not
  /// assert that the endpoint is local inference or grant any model permission.
  pub fn acquire(
    &self,
    model_id: &str,
    local_only: bool,
  ) -> Result<EndpointLease, EndpointAcquireError> {
    let now = tokio::time::Instant::now();
    let mut matching_capacity = false;
    for state in &self.endpoints {
      let endpoint = &state.config;
      if !endpoint.enabled
        || !endpoint.model_ids.contains(model_id)
        || (local_only
          && !Url::parse(&endpoint.api_base).is_ok_and(|url| is_literal_loopback(&url)))
      {
        continue;
      }
      if state
        .cooldown_until
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some_and(|until| until > now)
      {
        continue;
      }
      matching_capacity = true;
      if let Ok(permit) = Arc::clone(&state.permits).try_acquire_owned() {
        return Ok(EndpointLease {
          endpoint: endpoint.clone(),
          state: Arc::clone(state),
          _permit: permit,
        });
      }
    }
    if matching_capacity {
      Err(EndpointAcquireError::CapacityExhausted)
    } else {
      Err(EndpointAcquireError::Unavailable)
    }
  }
}

/// Explicit selection failures. Capacity exhaustion is separate from there
/// being no enabled, matching, non-cooled endpoint.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum EndpointAcquireError {
  #[error("no provider endpoint is available for this model")]
  Unavailable,
  #[error("all matching provider endpoints are at capacity")]
  CapacityExhausted,
}

/// A selected endpoint and its admission permit. Dropping the lease releases capacity.
pub struct EndpointLease {
  endpoint: ProviderEndpoint,
  state: Arc<EndpointState>,
  _permit: OwnedSemaphorePermit,
}

impl fmt::Debug for EndpointLease {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("EndpointLease")
      .field("endpoint_id", &self.endpoint.id)
      .finish_non_exhaustive()
  }
}

impl EndpointLease {
  pub fn endpoint(&self) -> &ProviderEndpoint {
    &self.endpoint
  }

  /// Applies a bounded process-local cooldown after an explicit rate-limit signal.
  pub fn mark_rate_limited(&self, duration: Duration) {
    self.mark_cooldown(duration);
  }

  /// Applies a bounded process-local cooldown after an explicit unavailable signal.
  pub fn mark_unavailable(&self, duration: Duration) {
    self.mark_cooldown(duration);
  }

  fn mark_cooldown(&self, duration: Duration) {
    let until = tokio::time::Instant::now() + duration.min(MAX_ENDPOINT_COOLDOWN);
    let mut cooldown = self
      .state
      .cooldown_until
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cooldown.is_none_or(|existing| existing < until) {
      *cooldown = Some(until);
    }
  }
}
