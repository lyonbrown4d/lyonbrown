//! Scope-checked memory orchestration. Persistence and index execution remain ports.
#![forbid(unsafe_code)]
mod embedding;
mod mutations;
mod retrieval;
mod worker;
pub use embedding::RigEmbedding;
use memory_core::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Constructed by authenticated server code, never inferred from model input.
#[derive(Clone, Debug)]
pub struct MemoryAccess {
  scopes: Vec<MemoryScope>,
}
impl MemoryAccess {
  pub fn new(scopes: Vec<MemoryScope>) -> Self {
    Self { scopes }
  }
  pub fn scopes(&self) -> &[MemoryScope] {
    &self.scopes
  }
  fn validate(&self) -> MemoryResult<()> {
    if self.scopes.is_empty() || self.scopes.len() > 100 {
      return Err(MemoryError::Forbidden);
    }
    Ok(())
  }
  fn check(&self, scope: &MemoryScope) -> MemoryResult<()> {
    self.validate()?;
    if !self.scopes.contains(scope) {
      return Err(MemoryError::Forbidden);
    }
    Ok(())
  }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetrievedMemory {
  pub item: MemoryItem,
  pub score: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryStatus {
  pub retrieval_ready: bool,
  pub embedding: Option<EmbeddingDescriptor>,
  pub vector: Option<VectorStatus>,
}
#[derive(Clone)]
pub struct MemoryService {
  repository: Arc<dyn MemoryRepository>,
  embedding: Option<Arc<dyn MemoryEmbedding>>,
  index: Option<Arc<dyn VectorIndex>>,
}
impl MemoryService {
  pub fn new(
    repository: Arc<dyn MemoryRepository>,
    embedding: Option<Arc<dyn MemoryEmbedding>>,
    index: Option<Arc<dyn VectorIndex>>,
  ) -> MemoryResult<Self> {
    if let Some(embedding) = &embedding {
      validate_descriptor(&embedding.descriptor())?;
    }
    Ok(Self {
      repository,
      embedding,
      index,
    })
  }
  pub async fn list(
    &self,
    access: &MemoryAccess,
    states: &[MemoryState],
    after: Option<&str>,
    limit: u32,
  ) -> MemoryResult<MemoryPage> {
    access.validate()?;
    if !(1..=100).contains(&limit) || states.len() > 5 || after.is_some_and(|id| id.len() > 128) {
      return Err(MemoryError::Invalid);
    }
    let page = self
      .repository
      .list(access.scopes(), states, after, limit)
      .await?;
    for item in &page.items {
      access.check(&item.scope)?;
    }
    Ok(page)
  }
  pub async fn get(&self, access: &MemoryAccess, id: &str) -> MemoryResult<Option<MemoryItem>> {
    access.validate()?;
    validate_id(id)?;
    let item = self.repository.get(id).await?;
    if let Some(item) = &item {
      access.check(&item.scope)?;
    }
    Ok(item)
  }
  pub async fn jobs(&self, access: &MemoryAccess, id: &str) -> MemoryResult<Vec<MemoryJob>> {
    self.get(access, id).await?.ok_or(MemoryError::NotFound)?;
    self.repository.jobs(id).await
  }
  pub async fn retry_job(
    &self,
    access: &MemoryAccess,
    memory_id: &str,
    job_id: &str,
  ) -> MemoryResult<()> {
    validate_id(job_id)?;
    let jobs = self.jobs(access, memory_id).await?;
    if !jobs
      .iter()
      .any(|job| job.id == job_id && job.memory_id == memory_id)
    {
      return Err(MemoryError::NotFound);
    }
    self.repository.retry_job(job_id).await
  }
  pub async fn bind_run(
    &self,
    access: &MemoryAccess,
    run: &str,
    session: &str,
    epoch: u64,
    items: &[MemoryItem],
  ) -> MemoryResult<()> {
    access.validate()?;
    if items.len() > 100 {
      return Err(MemoryError::Invalid);
    }
    for item in items {
      access.check(&item.scope)?;
    }
    self.repository.bind_run(run, session, epoch, items).await
  }
  pub async fn status(&self) -> MemoryResult<MemoryStatus> {
    let embedding = self.embedding.as_ref().map(|value| value.descriptor());
    let vector = if let Some(index) = &self.index {
      match index.status().await {
        Ok(status) => Some(VectorStatus {
          ready: status.ready,
          backend: safe_backend(&status.backend),
          detail: if status.ready { "ready" } else { "unavailable" }.into(),
        }),
        Err(_) => Some(VectorStatus {
          ready: false,
          backend: "configured".into(),
          detail: "unavailable".into(),
        }),
      }
    } else {
      None
    };
    Ok(MemoryStatus {
      retrieval_ready: embedding.is_some() && vector.as_ref().is_some_and(|value| value.ready),
      embedding,
      vector,
    })
  }
}
fn safe_backend(value: &str) -> String {
  if value.len() <= 64
    && value
      .bytes()
      .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-' | b'.'))
  {
    value.into()
  } else {
    "configured".into()
  }
}
fn validate_id(id: &str) -> MemoryResult<()> {
  if id.is_empty() || id.len() > 128 {
    return Err(MemoryError::Invalid);
  }
  Ok(())
}
fn validate_descriptor(value: &EmbeddingDescriptor) -> MemoryResult<()> {
  if value.dimensions == 0
    || value.dimensions > 65_536
    || [&value.id, &value.model, &value.configuration_revision]
      .iter()
      .any(|s| s.trim().is_empty() || s.len() > 256)
  {
    return Err(MemoryError::Invalid);
  }
  Ok(())
}
fn now_ms() -> MemoryResult<i64> {
  let duration = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_err(|_| MemoryError::Storage)?;
  i64::try_from(duration.as_millis()).map_err(|_| MemoryError::Storage)
}
fn valid_vector(vector: &[f32], descriptor: &EmbeddingDescriptor) -> bool {
  vector.len() == descriptor.dimensions as usize && vector.iter().all(|value| value.is_finite())
}
