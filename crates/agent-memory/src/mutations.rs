use crate::{MemoryAccess, MemoryService, now_ms};
use memory_core::*;
impl MemoryService {
  /// Explicit user edits may activate a memory. Model proposals have a separate entry.
  pub async fn save(
    &self,
    access: &MemoryAccess,
    mut mutation: MemoryMutation,
  ) -> MemoryResult<MemoryItem> {
    access.check(&mutation.item.scope)?;
    if mutation.item.content.trim().is_empty()
      || mutation.item.content.len() > 16_384
      || uuid::Uuid::parse_str(&mutation.item.id).is_err()
      || mutation.expected_revision.checked_add(1) != Some(mutation.item.revision)
      || mutation.item.state == MemoryState::Deleted
    {
      return Err(MemoryError::Invalid);
    }
    if mutation.item.state == MemoryState::Active
      && !matches!(mutation.source, MemorySource::Explicit { .. })
    {
      return Err(MemoryError::Forbidden);
    }
    if let MemorySource::Explicit { request_id } = &mutation.source
      && uuid::Uuid::parse_str(request_id).is_err()
    {
      return Err(MemoryError::Invalid);
    }
    let now = now_ms()?;
    let previous = self.get(access, &mutation.item.id).await?;
    if let Some(previous) = previous {
      // Let an earlier request reach durable dedup even after a later revision
      // becomes terminal. Fresh transitions also fail inside the repository CAS.
      let terminal_transition = mutation.expected_revision == previous.revision
        && matches!(
          previous.state,
          MemoryState::Rejected | MemoryState::Superseded
        )
        && mutation.item.state == MemoryState::Active;
      if previous.state == MemoryState::Deleted || terminal_transition {
        return Err(MemoryError::Conflict);
      }
      if previous.scope != mutation.item.scope {
        return Err(MemoryError::Forbidden);
      }
      mutation.item.created_at_ms = previous.created_at_ms;
      mutation.item.updated_at_ms = now.max(previous.updated_at_ms);
    } else {
      mutation.item.created_at_ms = now;
      mutation.item.updated_at_ms = now;
    }
    self.repository.save(mutation).await
  }
  pub async fn propose(
    &self,
    access: &MemoryAccess,
    mut mutation: MemoryMutation,
  ) -> MemoryResult<MemoryItem> {
    if !matches!(mutation.source, MemorySource::Session { .. }) {
      return Err(MemoryError::Forbidden);
    }
    if self
      .get(access, &mutation.item.id)
      .await?
      .is_some_and(|item| item.state != MemoryState::Proposed)
    {
      return Err(MemoryError::Conflict);
    }
    mutation.item.state = MemoryState::Proposed;
    self.save(access, mutation).await
  }
  pub async fn forget(
    &self,
    access: &MemoryAccess,
    id: &str,
    expected_revision: u64,
  ) -> MemoryResult<MemoryItem> {
    self.get(access, id).await?.ok_or(MemoryError::NotFound)?;
    self.repository.forget(id, expected_revision).await
  }
}
