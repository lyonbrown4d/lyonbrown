//! Application scope constraints expressed through Rig's filter vocabulary.
use crate::payload;
use memory_core::{IndexError, IndexResult, VectorQuery};
use rig_core::vector_store::request::SearchFilter;
use rig_qdrant::QdrantFilter;
use serde_json::json;

pub(crate) fn scoped(query: &VectorQuery) -> IndexResult<QdrantFilter> {
  let scopes = query
    .scopes
    .iter()
    .map(|scope| {
      let (kind, id) = payload::scope(scope);
      QdrantFilter::eq("scope_kind", json!(kind)).and(QdrantFilter::eq("scope_id", json!(id)))
    })
    .reduce(QdrantFilter::or)
    .ok_or(IndexError::Invalid)?;
  let embedding = &query.embedding;
  Ok(
    QdrantFilter::eq("embedding_id", json!(embedding.id))
      .and(QdrantFilter::eq("embedding_model", json!(embedding.model)))
      .and(QdrantFilter::eq(
        "embedding_dimensions",
        json!(embedding.dimensions),
      ))
      .and(QdrantFilter::eq(
        "embedding_configuration_revision",
        json!(embedding.configuration_revision),
      ))
      .and(scopes),
  )
}
