use crate::{MemoryAccess, MemoryService, RetrievedMemory, now_ms, valid_vector};
use futures::{StreamExt, stream};
use memory_core::*;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;
impl MemoryService {
  pub async fn search(
    &self,
    access: &MemoryAccess,
    text: &str,
    limit: u32,
    cancel: CancellationToken,
  ) -> MemoryResult<Vec<RetrievedMemory>> {
    access.validate()?;
    if text.trim().is_empty() || text.len() > 16_384 || !(1..=100).contains(&limit) {
      return Err(MemoryError::Invalid);
    }
    let embedding = self
      .embedding
      .as_ref()
      .ok_or(MemoryError::RetrievalUnavailable)?;
    let index = self
      .index
      .as_ref()
      .ok_or(MemoryError::RetrievalUnavailable)?;
    let vector = tokio::select! {biased; _=cancel.cancelled()=>return Err(MemoryError::Cancelled), result=embedding.embed(text,cancel.child_token())=>result?};
    let descriptor = embedding.descriptor();
    if !valid_vector(&vector, &descriptor) {
      return Err(MemoryError::EmbeddingUnavailable);
    }
    let query = VectorQuery {
      vector,
      scopes: access.scopes().to_vec(),
      embedding: descriptor,
      limit: limit.saturating_mul(4).min(100),
    };
    let hits = tokio::select! {biased; _=cancel.cancelled()=>return Err(MemoryError::Cancelled), result=index.search(query,cancel.child_token())=>result.map_err(|error|if matches!(error,IndexError::Cancelled){MemoryError::Cancelled}else{MemoryError::RetrievalUnavailable})?};
    let now = now_ms()?;
    // Bound both untrusted backend output and concurrent authoritative lookups.
    let mut candidates = stream::iter(
      hits
        .into_iter()
        .take(100)
        .filter(|hit| hit.score.is_finite() && hit.id.len() <= 128)
        .map(|hit| async move {
          let current = self.repository.get(&hit.id).await?;
          Ok::<_, MemoryError>((hit, current))
        }),
    )
    .buffer_unordered(8);
    let mut accepted: HashMap<String, RetrievedMemory> = HashMap::new();
    loop {
      let candidate = tokio::select! {biased; _=cancel.cancelled()=>return Err(MemoryError::Cancelled), candidate=candidates.next()=>candidate};
      let Some(candidate) = candidate else {
        break;
      };
      let (hit, item) = candidate?;
      let Some(item) = item else {
        continue;
      };
      if item.id != hit.id
        || item.revision != hit.revision
        || item.state != MemoryState::Active
        || !access.scopes().contains(&item.scope)
        || item.expires_at_ms.is_some_and(|expiry| expiry <= now)
      {
        continue;
      }
      if accepted
        .get(&item.id)
        .is_none_or(|current| hit.score > current.score)
      {
        accepted.insert(
          item.id.clone(),
          RetrievedMemory {
            item,
            score: hit.score,
          },
        );
      }
    }
    let mut accepted = accepted.into_values().collect::<Vec<_>>();
    accepted.sort_by(|left, right| {
      right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.item.id.cmp(&right.item.id))
    });
    accepted.truncate(limit as usize);
    Ok(accepted)
  }
}
