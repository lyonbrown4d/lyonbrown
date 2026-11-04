use agent_protocol::v1;
use memory_core::{self as core, MemoryError};
use tonic::Status;

pub(super) fn record(item: core::MemoryItem) -> v1::MemoryRecord {
  let (kind, id) = match item.scope {
    core::MemoryScope::User(id) => (v1::MemoryScopeKind::User, id),
    core::MemoryScope::Project(id) => (v1::MemoryScopeKind::Project, id),
    core::MemoryScope::Session(id) => (v1::MemoryScopeKind::Session, id),
  };
  v1::MemoryRecord {
    id: item.id,
    scope: Some(v1::MemoryScope {
      kind: kind as i32,
      id,
    }),
    kind: match item.kind {
      core::MemoryKind::Preference => v1::MemoryKind::Preference,
      core::MemoryKind::Fact => v1::MemoryKind::Fact,
      core::MemoryKind::Procedure => v1::MemoryKind::Procedure,
    } as i32,
    state: state_out(item.state) as i32,
    content: item.content,
    revision: item.revision,
    created_at_ms: item.created_at_ms,
    updated_at_ms: item.updated_at_ms,
    expires_at_ms: item.expires_at_ms,
  }
}
fn state_out(state: core::MemoryState) -> v1::MemoryState {
  match state {
    core::MemoryState::Proposed => v1::MemoryState::Proposed,
    core::MemoryState::Active => v1::MemoryState::Active,
    core::MemoryState::Superseded => v1::MemoryState::Superseded,
    core::MemoryState::Rejected => v1::MemoryState::Rejected,
    core::MemoryState::Deleted => v1::MemoryState::Deleted,
  }
}
pub(super) fn state(value: i32) -> Result<core::MemoryState, Status> {
  match v1::MemoryState::try_from(value) {
    Ok(v1::MemoryState::Proposed) => Ok(core::MemoryState::Proposed),
    Ok(v1::MemoryState::Active) => Ok(core::MemoryState::Active),
    Ok(v1::MemoryState::Superseded) => Ok(core::MemoryState::Superseded),
    Ok(v1::MemoryState::Rejected) => Ok(core::MemoryState::Rejected),
    Ok(v1::MemoryState::Deleted) => Ok(core::MemoryState::Deleted),
    _ => Err(Status::invalid_argument("invalid memory state")),
  }
}
pub(super) fn states(values: &[i32]) -> Result<Vec<core::MemoryState>, Status> {
  if values.is_empty() {
    return Ok(vec![
      core::MemoryState::Proposed,
      core::MemoryState::Active,
      core::MemoryState::Superseded,
      core::MemoryState::Rejected,
      core::MemoryState::Deleted,
    ]);
  }
  if values.len() > 5 {
    return Err(Status::invalid_argument("too many memory states"));
  }
  values.iter().copied().map(state).collect()
}
pub(super) fn put(
  input: v1::PutMemoryRequest,
  scope: core::MemoryScope,
) -> Result<core::MemoryMutation, Status> {
  if uuid::Uuid::parse_str(&input.id).is_err()
    || uuid::Uuid::parse_str(&input.request_id).is_err()
    || input.content.trim().is_empty()
    || input.content.len() > 16_384
    || input.expires_at_ms.is_some_and(|expiry| expiry <= 0)
  {
    return Err(Status::invalid_argument("invalid memory input"));
  }
  let state = state(input.state)?;
  if state == core::MemoryState::Deleted {
    return Err(Status::invalid_argument(
      "use forget_memory to delete a memory",
    ));
  }
  let kind = match v1::MemoryKind::try_from(input.kind) {
    Ok(v1::MemoryKind::Preference) => core::MemoryKind::Preference,
    Ok(v1::MemoryKind::Fact) => core::MemoryKind::Fact,
    Ok(v1::MemoryKind::Procedure) => core::MemoryKind::Procedure,
    _ => return Err(Status::invalid_argument("invalid memory kind")),
  };
  let revision = input
    .expected_revision
    .checked_add(1)
    .ok_or_else(|| Status::invalid_argument("memory revision exhausted"))?;
  Ok(core::MemoryMutation {
    item: core::MemoryItem {
      id: input.id,
      scope,
      kind,
      state,
      content: input.content,
      revision,
      created_at_ms: 0,
      updated_at_ms: 0,
      expires_at_ms: input.expires_at_ms,
    },
    expected_revision: input.expected_revision,
    source: core::MemorySource::Explicit {
      request_id: input.request_id,
    },
  })
}
pub(super) fn job(job: core::MemoryJob) -> v1::MemoryJob {
  v1::MemoryJob {
    id: job.id,
    memory_id: job.memory_id,
    revision: job.revision,
    kind: match job.kind {
      core::MemoryJobKind::Upsert => v1::MemoryJobKind::Upsert,
      core::MemoryJobKind::Delete => v1::MemoryJobKind::Delete,
    } as i32,
    state: match job.state {
      core::MemoryJobState::Pending => v1::MemoryJobState::Pending,
      core::MemoryJobState::Running => v1::MemoryJobState::Running,
      core::MemoryJobState::Completed => v1::MemoryJobState::Completed,
      core::MemoryJobState::Failed => v1::MemoryJobState::Failed,
      core::MemoryJobState::Uncertain => v1::MemoryJobState::Uncertain,
    } as i32,
    attempts: job.attempts,
    error_category: job.error_category,
  }
}
pub(super) fn status(value: agent_memory::MemoryStatus) -> v1::MemoryStatus {
  let (vector_backend, detail) = value
    .vector
    .map(|vector| (vector.backend, vector.detail))
    .unwrap_or_else(|| (String::new(), "not configured".into()));
  let (embedding_model, dimensions) = value
    .embedding
    .map(|embedding| (embedding.model, embedding.dimensions))
    .unwrap_or_default();
  v1::MemoryStatus {
    retrieval_ready: value.retrieval_ready,
    vector_backend,
    detail,
    embedding_model,
    dimensions,
  }
}
pub(super) fn failure(error: MemoryError) -> Status {
  match error {
    MemoryError::Invalid => Status::invalid_argument("invalid memory input"),
    MemoryError::NotFound => Status::not_found("memory not found"),
    MemoryError::Conflict => Status::aborted("memory revision or job attempt changed"),
    MemoryError::Forbidden => Status::permission_denied("memory scope is not authorized"),
    MemoryError::Storage => Status::unavailable("memory storage unavailable"),
    MemoryError::EmbeddingMismatch => {
      Status::failed_precondition("memory job requires its original embedding configuration")
    }
    MemoryError::EmbeddingUnavailable => Status::unavailable("memory embedding unavailable"),
    MemoryError::RetrievalUnavailable => Status::unavailable("memory retrieval unavailable"),
    MemoryError::Cancelled => Status::cancelled("memory operation cancelled"),
  }
}
