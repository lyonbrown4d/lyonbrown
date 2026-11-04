//! Memory domain and index ports. No ORM, Rig, provider or vector SDK types.
mod rebuild;
pub use rebuild::{MemoryRebuild, MemoryRebuildPage, MemoryRebuildState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
  Preference,
  Fact,
  Procedure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryState {
  Proposed,
  Active,
  Superseded,
  Rejected,
  Deleted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum MemoryScope {
  User(String),
  Project(String),
  Session(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryItem {
  pub id: String,
  pub scope: MemoryScope,
  pub kind: MemoryKind,
  pub state: MemoryState,
  pub content: String,
  pub revision: u64,
  pub created_at_ms: i64,
  pub updated_at_ms: i64,
  pub expires_at_ms: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingDescriptor {
  pub id: String,
  pub model: String,
  pub dimensions: u32,
  pub configuration_revision: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexedMemory {
  pub id: String,
  pub revision: u64,
  pub scope: MemoryScope,
  pub embedding: EmbeddingDescriptor,
  pub vector: Vec<f32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorQuery {
  pub vector: Vec<f32>,
  pub scopes: Vec<MemoryScope>,
  pub embedding: EmbeddingDescriptor,
  pub limit: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorHit {
  pub id: String,
  pub revision: u64,
  pub score: f32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorStatus {
  pub ready: bool,
  pub backend: String,
  pub detail: String,
}
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
  #[error("vector index input is invalid")]
  Invalid,
  #[error("vector index is unavailable")]
  Unavailable,
  #[error("vector operation was cancelled")]
  Cancelled,
  #[error("vector operation timed out")]
  Timeout,
  #[error("vector operation outcome is unconfirmed")]
  Uncertain,
  #[error("vector backend lacks required capability")]
  Unsupported,
}
pub type IndexResult<T> = Result<T, IndexError>;
#[async_trait::async_trait]
pub trait VectorIndex: Send + Sync {
  async fn upsert(
    &self,
    points: Vec<IndexedMemory>,
    cancel: tokio_util::sync::CancellationToken,
  ) -> IndexResult<()>;
  async fn delete(
    &self,
    id: &str,
    through_revision: u64,
    cancel: tokio_util::sync::CancellationToken,
  ) -> IndexResult<()>;
  async fn search(
    &self,
    query: VectorQuery,
    cancel: tokio_util::sync::CancellationToken,
  ) -> IndexResult<Vec<VectorHit>>;
  async fn status(&self) -> IndexResult<VectorStatus>;
  async fn maintain(&self, cancel: tokio_util::sync::CancellationToken) -> IndexResult<()>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemorySource {
  Explicit {
    request_id: String,
  },
  Session {
    session_id: String,
    run_id: String,
    event_cursor: u64,
    owner_epoch: u64,
  },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryMutation {
  pub item: MemoryItem,
  pub expected_revision: u64,
  pub source: MemorySource,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPage {
  pub items: Vec<MemoryItem>,
  pub next_cursor: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryJobKind {
  Upsert,
  Delete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryJobState {
  Pending,
  Running,
  Completed,
  Failed,
  Uncertain,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryJob {
  pub id: String,
  pub memory_id: String,
  pub revision: u64,
  pub kind: MemoryJobKind,
  pub state: MemoryJobState,
  pub attempts: u32,
  pub error_category: Option<String>,
}
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
  #[error("memory input is invalid")]
  Invalid,
  #[error("memory was not found")]
  NotFound,
  #[error("memory revision changed")]
  Conflict,
  #[error("memory scope is not authorized")]
  Forbidden,
  #[error("memory storage unavailable")]
  Storage,
  #[error("memory embedding is unavailable")]
  EmbeddingUnavailable,
  #[error("memory retrieval is unavailable")]
  RetrievalUnavailable,
  #[error("memory index job requires its original embedding configuration")]
  EmbeddingMismatch,
  #[error("memory operation was cancelled")]
  Cancelled,
}
pub type MemoryResult<T> = Result<T, MemoryError>;
#[async_trait::async_trait]
pub trait MemoryRepository: Send + Sync {
  async fn list(
    &self,
    scopes: &[MemoryScope],
    states: &[MemoryState],
    after: Option<&str>,
    limit: u32,
  ) -> MemoryResult<MemoryPage>;
  async fn get(&self, id: &str) -> MemoryResult<Option<MemoryItem>>;
  async fn save(&self, mutation: MemoryMutation) -> MemoryResult<MemoryItem>;
  async fn forget(&self, id: &str, expected_revision: u64) -> MemoryResult<MemoryItem>;
  async fn claim_job(&self) -> MemoryResult<Option<MemoryJob>>;
  async fn claim_job_for(&self, embedding: &EmbeddingDescriptor)
  -> MemoryResult<Option<MemoryJob>>;
  async fn finish_job(
    &self,
    id: &str,
    expected_attempt: u32,
    state: MemoryJobState,
    error_category: Option<&str>,
  ) -> MemoryResult<()>;
  async fn retry_job(&self, id: &str) -> MemoryResult<()>;
  async fn retry_job_for(
    &self,
    id: &str,
    embedding: Option<&EmbeddingDescriptor>,
  ) -> MemoryResult<()>;
  async fn jobs(&self, memory_id: &str) -> MemoryResult<Vec<MemoryJob>>;
  async fn recover_jobs(&self) -> MemoryResult<u64>;
  async fn project_scope(
    &self,
    owner: &str,
    canonical_workspace: &str,
  ) -> MemoryResult<MemoryScope>;
  async fn start_rebuild(
    &self,
    request_id: &str,
    scopes: &[MemoryScope],
    embedding: &EmbeddingDescriptor,
    force: bool,
    now_ms: i64,
  ) -> MemoryResult<MemoryRebuild>;
  async fn list_rebuilds(
    &self,
    scopes: &[MemoryScope],
    after: Option<&str>,
    limit: u32,
  ) -> MemoryResult<MemoryRebuildPage>;
  /// Atomically advances at most `limit` authoritative rows and enqueues their jobs.
  async fn advance_rebuild(
    &self,
    embedding: &EmbeddingDescriptor,
    limit: u32,
    now_ms: i64,
  ) -> MemoryResult<bool>;
  async fn bind_run(
    &self,
    run: &str,
    session: &str,
    owner_epoch: u64,
    memories: &[MemoryItem],
  ) -> MemoryResult<()>;
}
#[async_trait::async_trait]
pub trait MemoryEmbedding: Send + Sync {
  fn descriptor(&self) -> EmbeddingDescriptor;
  async fn embed(
    &self,
    text: &str,
    cancel: tokio_util::sync::CancellationToken,
  ) -> MemoryResult<Vec<f32>>;
}
