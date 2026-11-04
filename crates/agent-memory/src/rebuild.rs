use crate::{MemoryAccess, MemoryService, now_ms};
use memory_core::*;
impl MemoryService {
  /// A descriptor change creates a new durable scan; identical configuration is reused.
  pub async fn ensure_rebuild(&self, access: &MemoryAccess) -> MemoryResult<Option<MemoryRebuild>> {
    if self.embedding.is_none() || self.index.is_none() {
      return Ok(None);
    }
    self
      .start_rebuild(access, &uuid::Uuid::now_v7().to_string(), false)
      .await
      .map(Some)
  }
  pub async fn start_rebuild(
    &self,
    access: &MemoryAccess,
    request_id: &str,
    force: bool,
  ) -> MemoryResult<MemoryRebuild> {
    access.validate()?;
    if uuid::Uuid::parse_str(request_id).is_err() {
      return Err(MemoryError::Invalid);
    }
    let embedding = self
      .embedding
      .as_ref()
      .ok_or(MemoryError::EmbeddingUnavailable)?;
    if self.index.is_none() {
      return Err(MemoryError::RetrievalUnavailable);
    }
    self
      .repository
      .start_rebuild(
        request_id,
        access.scopes(),
        &embedding.descriptor(),
        force,
        now_ms()?,
      )
      .await
  }
  pub async fn rebuilds(
    &self,
    access: &MemoryAccess,
    after: Option<&str>,
    limit: u32,
  ) -> MemoryResult<MemoryRebuildPage> {
    access.validate()?;
    if !(1..=100).contains(&limit) || after.is_some_and(|id| id.len() > 128) {
      return Err(MemoryError::Invalid);
    }
    let page = self
      .repository
      .list_rebuilds(access.scopes(), after, limit)
      .await?;
    if page.items.iter().any(|item| item.scopes != access.scopes()) {
      return Err(MemoryError::Forbidden);
    }
    Ok(page)
  }
  /// Model reads use only active, current, unexpired authoritative records.
  pub async fn read_active(
    &self,
    access: &MemoryAccess,
    id: &str,
  ) -> MemoryResult<Option<MemoryItem>> {
    let now = now_ms()?;
    Ok(self.get(access, id).await?.filter(|item| {
      item.state == MemoryState::Active && item.expires_at_ms.is_none_or(|expiry| expiry > now)
    }))
  }
}
