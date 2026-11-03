//! Authoritative memory state and durable index work; no index SDK is stored here.
mod bindings;
mod entities;
mod jobs;
mod project_scope;
mod read;
mod rebuilds;
mod validation;
mod write;
use crate::Store;
use memory_core::*;
use sha2::{Digest, Sha256};

fn storage_error(error: anyhow::Error) -> MemoryError {
  error
    .downcast::<MemoryError>()
    .unwrap_or(MemoryError::Storage)
}
fn digest(value: &[u8]) -> String {
  hex::encode(Sha256::digest(value))
}
fn scope_key(scope: &MemoryScope) -> anyhow::Result<String> {
  Ok(digest(&serde_json::to_vec(scope)?))
}
fn state_name(state: MemoryState) -> &'static str {
  match state {
    MemoryState::Proposed => "proposed",
    MemoryState::Active => "active",
    MemoryState::Superseded => "superseded",
    MemoryState::Rejected => "rejected",
    MemoryState::Deleted => "deleted",
  }
}
fn job_state(state: MemoryJobState) -> &'static str {
  match state {
    MemoryJobState::Pending => "pending",
    MemoryJobState::Running => "running",
    MemoryJobState::Completed => "completed",
    MemoryJobState::Failed => "failed",
    MemoryJobState::Uncertain => "uncertain",
  }
}
#[async_trait::async_trait]
impl MemoryRepository for Store {
  async fn list(
    &self,
    scopes: &[MemoryScope],
    states: &[MemoryState],
    after: Option<&str>,
    limit: u32,
  ) -> MemoryResult<MemoryPage> {
    self
      .memory_list(scopes, states, after, limit)
      .await
      .map_err(storage_error)
  }
  async fn get(&self, id: &str) -> MemoryResult<Option<MemoryItem>> {
    self.memory_get(id).await.map_err(storage_error)
  }
  async fn save(&self, mutation: MemoryMutation) -> MemoryResult<MemoryItem> {
    self.memory_save(mutation).await.map_err(storage_error)
  }
  async fn forget(&self, id: &str, expected_revision: u64) -> MemoryResult<MemoryItem> {
    self
      .memory_forget(id, expected_revision)
      .await
      .map_err(storage_error)
  }
  async fn claim_job(&self) -> MemoryResult<Option<MemoryJob>> {
    self.memory_claim_job(None).await.map_err(storage_error)
  }
  async fn claim_job_for(
    &self,
    embedding: &EmbeddingDescriptor,
  ) -> MemoryResult<Option<MemoryJob>> {
    self
      .memory_claim_job(Some(embedding))
      .await
      .map_err(storage_error)
  }
  async fn finish_job(
    &self,
    id: &str,
    expected_attempt: u32,
    state: MemoryJobState,
    error_category: Option<&str>,
  ) -> MemoryResult<()> {
    self
      .memory_finish_job(id, expected_attempt, state, error_category)
      .await
      .map_err(storage_error)
  }
  async fn jobs(&self, id: &str) -> MemoryResult<Vec<MemoryJob>> {
    self.memory_jobs(id).await.map_err(storage_error)
  }
  async fn retry_job(&self, id: &str) -> MemoryResult<()> {
    self
      .memory_retry_job(id, None, false)
      .await
      .map_err(storage_error)
  }
  async fn retry_job_for(
    &self,
    id: &str,
    embedding: Option<&EmbeddingDescriptor>,
  ) -> MemoryResult<()> {
    self
      .memory_retry_job(id, embedding, true)
      .await
      .map_err(storage_error)
  }
  async fn recover_jobs(&self) -> MemoryResult<u64> {
    self.memory_recover_jobs().await.map_err(storage_error)
  }
  async fn project_scope(
    &self,
    owner: &str,
    canonical_workspace: &str,
  ) -> MemoryResult<MemoryScope> {
    self
      .memory_project_scope(owner, canonical_workspace)
      .await
      .map_err(storage_error)
  }
  async fn start_rebuild(
    &self,
    id: &str,
    scopes: &[MemoryScope],
    embedding: &EmbeddingDescriptor,
    force: bool,
    now: i64,
  ) -> MemoryResult<MemoryRebuild> {
    self
      .memory_start_rebuild(id, scopes, embedding, force, now)
      .await
      .map_err(storage_error)
  }
  async fn list_rebuilds(
    &self,
    scopes: &[MemoryScope],
    after: Option<&str>,
    limit: u32,
  ) -> MemoryResult<MemoryRebuildPage> {
    self
      .memory_list_rebuilds(scopes, after, limit)
      .await
      .map_err(storage_error)
  }
  async fn advance_rebuild(
    &self,
    embedding: &EmbeddingDescriptor,
    limit: u32,
    now: i64,
  ) -> MemoryResult<bool> {
    self
      .memory_advance_rebuild(embedding, limit, now)
      .await
      .map_err(storage_error)
  }
  async fn bind_run(
    &self,
    run: &str,
    session: &str,
    epoch: u64,
    memories: &[MemoryItem],
  ) -> MemoryResult<()> {
    self
      .memory_bind_run(run, session, epoch, memories)
      .await
      .map_err(storage_error)
  }
}
