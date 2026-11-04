use super::{descriptor_key, rebuilds};
use crate::{
  Store, event_tx,
  memory::{
    entities::{items, jobs},
    scope_key,
  },
};
use anyhow::Result;
use memory_core::*;
use sea_orm::{Condition, PaginatorTrait, QueryOrder, QuerySelect, QueryTrait, prelude::*};

impl Store {
  pub(in crate::memory) async fn memory_advance_rebuild(
    &self,
    embedding: &EmbeddingDescriptor,
    limit: u32,
    now: i64,
  ) -> Result<bool> {
    if !(1..=100).contains(&limit) {
      return Err(MemoryError::Invalid.into());
    }
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let unfinished = jobs::Entity::find().select_only().column(jobs::Column::RebuildId)
        .filter(jobs::Column::RebuildId.is_not_null()).filter(jobs::Column::State.ne("completed"))
        .into_query();
      let Some(row) = rebuilds::Entity::find()
        .filter(Condition::any().add(rebuilds::Column::ScanFinished.eq(false))
          .add(rebuilds::Column::Id.not_in_subquery(unfinished)))
        .filter(rebuilds::Column::State.eq(1_i16))
        .filter(rebuilds::Column::DescriptorKey.eq(descriptor_key(embedding)?))
        .order_by_asc(rebuilds::Column::Id).one(&transaction).await? else { return Ok(false); };
      let mut rebuild: MemoryRebuild = serde_json::from_str(&row.record)?;
      if rebuild.embedding != *embedding { return Err(MemoryError::Conflict.into()); }
      let mut advanced = false;
      if !rebuild.scan_finished {
        let keys = rebuild.scopes.iter().map(scope_key).collect::<Result<Vec<_>>>()?;
        let mut query = items::Entity::find().filter(items::Column::ScopeKey.is_in(keys));
        if let Some(after) = &rebuild.after_id { query = query.filter(items::Column::Id.gt(after)); }
        let mut rows = query.order_by_asc(items::Column::Id).limit(u64::from(limit) + 1)
          .all(&transaction).await?;
        rebuild.scan_finished = rows.len() <= limit as usize;
        rows.truncate(limit as usize);
        for row in rows {
          let item: MemoryItem = serde_json::from_str(&row.record)?;
          if !rebuild.scopes.contains(&item.scope) { return Err(MemoryError::Forbidden.into()); }
          super::super::jobs::enqueue_rebuild(&transaction, &item, Some(&rebuild.id), Some(&rebuild.embedding)).await?;
          rebuild.after_id = Some(item.id);
          rebuild.scanned = rebuild.scanned.checked_add(1).ok_or(MemoryError::Storage)?;
          rebuild.enqueued = rebuild.enqueued.checked_add(1).ok_or(MemoryError::Storage)?;
        }
        advanced = true;
      }
      if rebuild.scan_finished {
        let unfinished = jobs::Entity::find().filter(jobs::Column::RebuildId.eq(&rebuild.id))
          .filter(jobs::Column::State.ne("completed")).count(&transaction).await?;
        if unfinished == 0 {
          rebuild.state = MemoryRebuildState::Completed;
          advanced = true;
        }
      }
      if !advanced { return Ok(false); }
      rebuild.updated_at_ms = now;
      let updated = rebuilds::Entity::update_many().filter(rebuilds::Column::Id.eq(&rebuild.id))
        .filter(rebuilds::Column::State.eq(1_i16))
        .col_expr(rebuilds::Column::State, Expr::value(if rebuild.state == MemoryRebuildState::Completed {2_i16} else {1_i16}))
        .col_expr(rebuilds::Column::ScanFinished, Expr::value(rebuild.scan_finished))
        .col_expr(rebuilds::Column::Record, Expr::value(serde_json::to_string(&rebuild)?))
        .exec(&transaction).await?;
      if updated.rows_affected != 1 { return Err(MemoryError::Conflict.into()); }
      event_tx(&transaction, &format!("memory_rebuild:{}",rebuild.id), "memory_rebuild_advanced",
        serde_json::json!({"rebuild_id":rebuild.id,"scanned":rebuild.scanned,"enqueued":rebuild.enqueued,"state":rebuild.state})).await?;
      Ok(true)
    }.await;
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
}
