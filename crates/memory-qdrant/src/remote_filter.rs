use crate::payload;
use memory_core::VectorQuery;
use qdrant_client::qdrant::{Condition, Filter, Range};

pub(crate) fn filter(query: &VectorQuery) -> Filter {
  let embedding = &query.embedding;
  let scopes = query
    .scopes
    .iter()
    .map(|scope| {
      let (kind, id) = payload::scope(scope);
      Condition::from(Filter::must([
        Condition::matches("scope_kind", kind.to_owned()),
        Condition::matches("scope_id", id.to_owned()),
      ]))
    })
    .collect::<Vec<_>>();
  Filter::must([
    Condition::matches("embedding_id", embedding.id.clone()),
    Condition::matches("embedding_model", embedding.model.clone()),
    Condition::matches("embedding_dimensions", embedding.dimensions as i64),
    Condition::matches(
      "embedding_configuration_revision",
      embedding.configuration_revision.clone(),
    ),
    Condition::from(Filter::should(scopes)),
  ])
}

pub(crate) fn deletion(id: &str, through_revision: u64) -> Filter {
  let hi = (through_revision >> 32) as u32;
  let lo = through_revision as u32;
  // Qdrant numeric ranges are f64. The exact 32-bit components prevent a
  // rounded i64 cutoff from deleting the next revision above 2^53.
  Filter::must([
    Condition::matches("memory_id", id.to_owned()),
    Condition::range(
      "content_revision",
      Range {
        lte: Some(payload::revision_upper_bound(through_revision)),
        ..Default::default()
      },
    ),
    Condition::from(Filter::should([
      Condition::range(
        "content_revision_hi",
        Range {
          lt: Some(hi as f64),
          ..Default::default()
        },
      ),
      Condition::from(Filter::must([
        Condition::matches("content_revision_hi", hi as i64),
        Condition::range(
          "content_revision_lo",
          Range {
            lte: Some(lo as f64),
            ..Default::default()
          },
        ),
      ])),
    ])),
  ])
}
