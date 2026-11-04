mod progress;
use super::{digest, entities::rebuilds};
use crate::{Store, event_tx};
use anyhow::Result;
use memory_core::*;
use sea_orm::{QueryOrder, QuerySelect, Set, prelude::*};

pub(super) fn descriptor_key(value: &EmbeddingDescriptor) -> Result<String> {
  Ok(digest(&serde_json::to_vec(value)?))
}
fn scopes_key(scopes: &[MemoryScope]) -> Result<String> {
  Ok(digest(&serde_json::to_vec(scopes)?))
}
impl Store {
  pub(in crate::memory) async fn memory_start_rebuild(
    &self,
    request_id: &str,
    scopes: &[MemoryScope],
    embedding: &EmbeddingDescriptor,
    force: bool,
    now: i64,
  ) -> Result<MemoryRebuild> {
    if uuid::Uuid::parse_str(request_id).is_err() || scopes.is_empty() || scopes.len() > 100 {
      return Err(MemoryError::Invalid.into());
    }
    let scope_key = scopes_key(scopes)?;
    let descriptor_key = descriptor_key(embedding)?;
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      if let Some(existing) = rebuilds::Entity::find_by_id(request_id)
        .one(&transaction)
        .await?
      {
        let record: MemoryRebuild = serde_json::from_str(&existing.record)?;
        if record.scopes != scopes || record.embedding != *embedding {
          return Err(MemoryError::Conflict.into());
        }
        return Ok(record);
      }
      let latest = rebuilds::Entity::find()
        .filter(rebuilds::Column::ScopeKey.eq(&scope_key))
        .order_by_desc(rebuilds::Column::Generation)
        .one(&transaction)
        .await?;
      if !force && let Some(existing) = &latest {
        let record: MemoryRebuild = serde_json::from_str(&existing.record)?;
        if record.scopes != scopes {
          return Err(MemoryError::Conflict.into());
        }
        if record.embedding == *embedding {
          return Ok(record);
        }
      }
      let generation = latest
        .map_or(Some(1), |row| row.generation.checked_add(1))
        .ok_or(MemoryError::Storage)?;
      let rebuild = MemoryRebuild {
        id: request_id.into(),
        scopes: scopes.into(),
        embedding: embedding.clone(),
        state: MemoryRebuildState::Pending,
        after_id: None,
        scan_finished: false,
        scanned: 0,
        enqueued: 0,
        created_at_ms: now,
        updated_at_ms: now,
      };
      rebuilds::ActiveModel {
        id: Set(request_id.into()),
        scope_key: Set(scope_key),
        descriptor_key: Set(descriptor_key),
        scan_finished: Set(false),
        generation: Set(generation),
        state: Set(1),
        record: Set(serde_json::to_string(&rebuild)?),
      }
      .insert(&transaction)
      .await?;
      event_tx(
        &transaction,
        &format!("memory_rebuild:{request_id}"),
        "memory_rebuild_requested",
        serde_json::json!({"rebuild_id":request_id,"state":"pending"}),
      )
      .await?;
      Ok(rebuild)
    }
    .await;
    match result {
      Ok(value) => {
        transaction.commit().await?;
        Ok(value)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }

  pub(in crate::memory) async fn memory_list_rebuilds(
    &self,
    scopes: &[MemoryScope],
    after: Option<&str>,
    limit: u32,
  ) -> Result<MemoryRebuildPage> {
    if scopes.is_empty() || scopes.len() > 100 || !(1..=100).contains(&limit) {
      return Err(MemoryError::Invalid.into());
    }
    let mut query =
      rebuilds::Entity::find().filter(rebuilds::Column::ScopeKey.eq(scopes_key(scopes)?));
    if let Some(after) = after {
      let cursor = rebuilds::Entity::find_by_id(after)
        .one(self.connection())
        .await?
        .ok_or(MemoryError::Invalid)?;
      if cursor.scope_key != scopes_key(scopes)? {
        return Err(MemoryError::Forbidden.into());
      }
      query = query.filter(rebuilds::Column::Generation.lt(cursor.generation));
    }
    let mut rows = query
      .order_by_desc(rebuilds::Column::Generation)
      .limit(u64::from(limit) + 1)
      .all(self.connection())
      .await?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let items = rows
      .into_iter()
      .map(|row| serde_json::from_str(&row.record))
      .collect::<std::result::Result<Vec<MemoryRebuild>, _>>()?;
    if items.iter().any(|item| item.scopes != scopes) {
      return Err(MemoryError::Forbidden.into());
    }
    let next_cursor = more
      .then(|| items.last().map(|item| item.id.clone()))
      .flatten();
    Ok(MemoryRebuildPage { items, next_cursor })
  }
}
