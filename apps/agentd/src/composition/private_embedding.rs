//! Embedding cannot consume private values until a stable index projection is defined.
use std::sync::Arc;

use agent_privacy::{Detector, RuleDetector};
use agent_secrets::{SecretStore, SecretString};
use memory_core::{EmbeddingDescriptor, MemoryEmbedding, MemoryError, MemoryResult};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
#[path = "../../tests/internal/private_embedding.rs"]
mod tests;

pub(super) struct PrivateEmbedding {
  inner: Arc<dyn MemoryEmbedding>,
  known: Vec<SecretString>,
  secrets: Arc<SecretStore>,
  permits: Arc<Semaphore>,
}

impl PrivateEmbedding {
  pub fn new(
    inner: Arc<dyn MemoryEmbedding>,
    config: &crate::config::Config,
    secrets: Arc<SecretStore>,
    extra_known: &[SecretString],
  ) -> anyhow::Result<Self> {
    let mut known = config.privacy_credentials();
    known.extend_from_slice(extra_known);
    RuleDetector::new(&known)?;
    Ok(Self {
      inner,
      known,
      secrets,
      permits: Arc::new(Semaphore::new(1)),
    })
  }
}

#[async_trait::async_trait]
impl MemoryEmbedding for PrivateEmbedding {
  fn descriptor(&self) -> EmbeddingDescriptor {
    self.inner.descriptor()
  }

  async fn embed(&self, text: &str, cancel: CancellationToken) -> MemoryResult<Vec<f32>> {
    let permit = self
      .permits
      .clone()
      .try_acquire_owned()
      .map_err(|_| MemoryError::EmbeddingUnavailable)?;
    let mut known = self.known.clone();
    let secrets = self.secrets.clone();
    let input = text.to_owned();
    let detection = tokio::task::spawn_blocking(move || -> MemoryResult<bool> {
      let _permit = permit;
      let metadata = secrets
        .list()
        .map_err(|_| MemoryError::EmbeddingUnavailable)?;
      if metadata.len() > 1024 {
        return Err(MemoryError::EmbeddingUnavailable);
      }
      for item in metadata {
        let value = secrets
          .get(&item.id)
          .map_err(|_| MemoryError::EmbeddingUnavailable)?
          .ok_or(MemoryError::EmbeddingUnavailable)?;
        known.push(value);
      }
      let detector = RuleDetector::new(&known).map_err(|_| MemoryError::EmbeddingUnavailable)?;
      let findings = detector
        .detect(&input)
        .map_err(|_| MemoryError::EmbeddingUnavailable)?;
      Ok(findings.is_empty())
    });
    let safe = tokio::select! {
      biased;
      _ = cancel.cancelled() => return Err(MemoryError::Cancelled),
      result = detection => result.map_err(|_| MemoryError::EmbeddingUnavailable)??,
    };
    if !safe {
      // Random session references would change vector semantics across restarts.
      // Refuse this egress until an explicit stable privacy projection is configured.
      return Err(MemoryError::EmbeddingUnavailable);
    }
    self.inner.embed(text, cancel).await
  }
}
