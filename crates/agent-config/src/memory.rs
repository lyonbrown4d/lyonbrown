//! Memory configuration contains credential references, never credential values.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySettings {
  #[serde(default = "enabled")]
  pub enabled: bool,
  #[serde(default)]
  pub vector: MemoryVectorSettings,
  #[serde(default)]
  pub embedding: Option<MemoryEmbeddingSettings>,
  #[serde(default = "context_budget")]
  pub context_bytes: usize,
}
fn enabled() -> bool {
  true
}
fn context_budget() -> usize {
  8192
}
impl Default for MemorySettings {
  fn default() -> Self {
    Self {
      enabled: true,
      vector: Default::default(),
      embedding: None,
      context_bytes: context_budget(),
    }
  }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryVectorSettings {
  Embedded {
    directory: Option<PathBuf>,
  },
  Qdrant {
    endpoint: String,
    collection: String,
    api_key_env: Option<String>,
  },
}
impl Default for MemoryVectorSettings {
  fn default() -> Self {
    Self::Embedded { directory: None }
  }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEmbeddingSettings {
  pub id: String,
  pub model: String,
  pub dimensions: u32,
  pub api_base: String,
  pub api_key_env: Option<String>,
}
impl MemorySettings {
  pub fn credential_references(&self) -> Vec<(&'static str, &str)> {
    if !self.enabled || self.embedding.is_none() {
      return Vec::new();
    }
    let mut names = Vec::new();
    if let MemoryVectorSettings::Qdrant {
      api_key_env: Some(name),
      ..
    } = &self.vector
    {
      names.push(("memory_vector", name.as_str()));
    }
    if let Some(MemoryEmbeddingSettings {
      api_key_env: Some(name),
      ..
    }) = &self.embedding
    {
      names.push(("memory_embedding", name.as_str()));
    }
    names
  }
  pub fn validate(&self) -> Result<()> {
    ensure!(
      (1024..=65536).contains(&self.context_bytes),
      "memory context budget must be 1024..=65536 bytes"
    );
    if let Some(embedding) = &self.embedding {
      ensure!(
        !embedding.id.is_empty()
          && embedding.id.len() <= 128
          && !embedding.model.is_empty()
          && embedding.model.len() <= 256
          && (1..=65536).contains(&embedding.dimensions),
        "invalid memory embedding configuration"
      );
      endpoint(&embedding.api_base)?;
    }
    if let MemoryVectorSettings::Qdrant {
      endpoint: address,
      collection,
      ..
    } = &self.vector
    {
      endpoint(address)?;
      ensure!(
        !collection.is_empty()
          && collection.len() <= 128
          && collection
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "invalid memory collection name"
      );
    }
    for (_, name) in self.credential_references() {
      ensure!(
        !name.is_empty()
          && name.len() <= 128
          && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "invalid memory credential reference"
      );
    }
    Ok(())
  }
}
fn endpoint(value: &str) -> Result<()> {
  let parsed = url::Url::parse(value).map_err(|_| anyhow::anyhow!("invalid memory endpoint"))?;
  let local = match parsed.host() {
    Some(url::Host::Domain(name)) => name == "localhost",
    Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
    Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
    _ => false,
  };
  ensure!(
    value.len() <= 2048
      && !value.chars().any(char::is_whitespace)
      && parsed.host().is_some()
      && parsed.username().is_empty()
      && parsed.password().is_none()
      && parsed.query().is_none()
      && parsed.fragment().is_none()
      && (parsed.scheme() == "https" || (parsed.scheme() == "http" && local)),
    "memory endpoint requires HTTPS or loopback HTTP without embedded credentials"
  );
  Ok(())
}
