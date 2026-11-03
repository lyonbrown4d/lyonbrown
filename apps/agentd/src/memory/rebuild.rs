use agent_protocol::v1;
use memory_core::{MemoryRebuild, MemoryRebuildState, MemoryScope};
pub(super) fn record(value: MemoryRebuild) -> v1::MemoryRebuild {
  v1::MemoryRebuild {
    id: value.id,
    scopes: value
      .scopes
      .into_iter()
      .map(|scope| {
        let (kind, id) = match scope {
          MemoryScope::User(id) => (v1::MemoryScopeKind::User, id),
          MemoryScope::Project(id) => (v1::MemoryScopeKind::Project, id),
          MemoryScope::Session(id) => (v1::MemoryScopeKind::Session, id),
        };
        v1::MemoryScope {
          kind: kind as i32,
          id,
        }
      })
      .collect(),
    embedding_id: value.embedding.id,
    embedding_model: value.embedding.model,
    dimensions: value.embedding.dimensions,
    configuration_revision: value.embedding.configuration_revision,
    state: match value.state {
      MemoryRebuildState::Pending => v1::MemoryRebuildState::Pending,
      MemoryRebuildState::Completed => v1::MemoryRebuildState::Completed,
    } as i32,
    after_id: value.after_id.unwrap_or_default(),
    scanned: value.scanned,
    enqueued: value.enqueued,
    created_at_ms: value.created_at_ms,
    updated_at_ms: value.updated_at_ms,
  }
}
