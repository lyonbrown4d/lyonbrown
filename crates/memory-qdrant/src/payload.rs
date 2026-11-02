use memory_core::{
  EmbeddingDescriptor, IndexError, IndexResult, IndexedMemory, MemoryScope, VectorHit, VectorQuery,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const VECTOR: &str = "memory";
pub(crate) const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

// Numeric range APIs use f64 even for integer indexes. Round the coarse bound
// outwards; the hi/lo predicate supplies the exact integer cutoff.
pub(crate) fn revision_upper_bound(revision: u64) -> f64 {
  (revision as f64).next_up()
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Payload {
  pub memory_id: String,
  pub content_revision: u64,
  pub content_revision_hi: u32,
  pub content_revision_lo: u32,
  pub scope_kind: String,
  pub scope_id: String,
  pub embedding_id: String,
  pub embedding_model: String,
  pub embedding_dimensions: u32,
  pub embedding_configuration_revision: String,
}

pub(crate) fn text(value: &str) -> bool {
  !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

pub(crate) fn descriptor(value: &EmbeddingDescriptor) -> IndexResult<()> {
  if value.dimensions == 0
    || value.dimensions > 65536
    || !text(&value.id)
    || !text(&value.model)
    || !text(&value.configuration_revision)
  {
    return Err(IndexError::Invalid);
  }
  Ok(())
}

pub(crate) fn scope(value: &MemoryScope) -> (&'static str, &str) {
  match value {
    MemoryScope::User(id) => ("user", id),
    MemoryScope::Project(id) => ("project", id),
    MemoryScope::Session(id) => ("session", id),
  }
}

fn vector(value: &[f32], expected: &EmbeddingDescriptor) -> bool {
  value.len() == expected.dimensions as usize
    && value.iter().all(|n| n.is_finite())
    && value.iter().any(|n| *n != 0.0)
}

pub(crate) fn points(values: &[IndexedMemory], expected: &EmbeddingDescriptor) -> IndexResult<()> {
  if values.is_empty() || values.len() > 256 {
    return Err(IndexError::Invalid);
  }
  for value in values {
    id(&value.id)?;
    if value.revision == 0
      || value.revision > i64::MAX as u64
      || value.embedding != *expected
      || !text(scope(&value.scope).1)
      || !vector(&value.vector, expected)
    {
      return Err(IndexError::Invalid);
    }
  }
  Ok(())
}

pub(crate) fn query(value: &VectorQuery, expected: &EmbeddingDescriptor) -> IndexResult<()> {
  if value.embedding != *expected
    || !vector(&value.vector, expected)
    || !(1..=100).contains(&value.limit)
    || value.scopes.is_empty()
    || value.scopes.len() > 64
    || value.scopes.iter().any(|s| !text(scope(s).1))
  {
    return Err(IndexError::Invalid);
  }
  Ok(())
}

pub(crate) fn id(value: &str) -> IndexResult<()> {
  Uuid::parse_str(value)
    .map(|_| ())
    .map_err(|_| IndexError::Invalid)
}

pub(crate) fn point_id(value: &IndexedMemory) -> IndexResult<Uuid> {
  // Immutable revision/configuration IDs keep delayed work from overwriting a newer revision.
  let identity = serde_json::to_vec(&(&value.id, value.revision, &value.embedding))
    .map_err(|_| IndexError::Invalid)?;
  let hash = Sha256::digest(identity);
  let mut bytes = [0; 16];
  bytes.copy_from_slice(&hash[..16]);
  Ok(Uuid::from_bytes(bytes))
}

pub(crate) fn payload(value: &IndexedMemory) -> Payload {
  let (kind, id) = scope(&value.scope);
  Payload {
    memory_id: value.id.clone(),
    content_revision: value.revision,
    content_revision_hi: (value.revision >> 32) as u32,
    content_revision_lo: value.revision as u32,
    scope_kind: kind.into(),
    scope_id: id.into(),
    embedding_id: value.embedding.id.clone(),
    embedding_model: value.embedding.model.clone(),
    embedding_dimensions: value.embedding.dimensions,
    embedding_configuration_revision: value.embedding.configuration_revision.clone(),
  }
}

pub(crate) fn fields(value: &EmbeddingDescriptor) -> Vec<(&'static str, serde_json::Value)> {
  vec![
    ("embedding_id", value.id.clone().into()),
    ("embedding_model", value.model.clone().into()),
    ("embedding_dimensions", value.dimensions.into()),
    (
      "embedding_configuration_revision",
      value.configuration_revision.clone().into(),
    ),
  ]
}

pub(crate) fn hit(value: serde_json::Value, score: f32) -> IndexResult<VectorHit> {
  let value: Payload = serde_json::from_value(value).map_err(|_| IndexError::Unavailable)?;
  if !score.is_finite() || value.content_revision == 0 || value.content_revision > i64::MAX as u64 {
    return Err(IndexError::Unavailable);
  }
  id(&value.memory_id).map_err(|_| IndexError::Unavailable)?;
  Ok(VectorHit {
    id: value.memory_id,
    revision: value.content_revision,
    score,
  })
}
