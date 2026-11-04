//! Rebuilds enqueue bounded durable index work; they never grant a new scope.
use crate::{EmbeddingDescriptor, MemoryScope};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRebuildState {
  Pending,
  Completed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRebuild {
  pub id: String,
  pub scopes: Vec<MemoryScope>,
  pub embedding: EmbeddingDescriptor,
  pub state: MemoryRebuildState,
  pub after_id: Option<String>,
  pub scan_finished: bool,
  pub scanned: u64,
  pub enqueued: u64,
  pub created_at_ms: i64,
  pub updated_at_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRebuildPage {
  pub items: Vec<MemoryRebuild>,
  pub next_cursor: Option<String>,
}
