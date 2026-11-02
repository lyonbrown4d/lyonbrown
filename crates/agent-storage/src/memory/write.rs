use super::validation::{validate, validate_source};
use super::{
  digest,
  entities::{items, requests, revisions},
  scope_key, state_name,
};
use crate::{Store, event_tx};
use anyhow::Result;
use memory_core::{MemoryError, MemoryItem, MemoryMutation, MemorySource, MemoryState};
use sea_orm::{Set, prelude::*};

impl Store {
  pub(super) async fn memory_save(&self, mutation: MemoryMutation) -> Result<MemoryItem> {
    validate(&mutation)?;
    // Server timestamps are bookkeeping, not part of the caller's retry body.
    let mut request_body = mutation.clone();
    request_body.item.created_at_ms = 0;
    request_body.item.updated_at_ms = 0;
    let body_hash = digest(&serde_json::to_vec(&request_body)?);
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      // Dedup is checked under the same database fence as CAS and index intent.
      if let MemorySource::Explicit { request_id } = &mutation.source {
        if let Some(request) = requests::Entity::find_by_id(request_id)
          .one(&transaction)
          .await?
        {
          if request.body_hash != body_hash {
            return Err(MemoryError::Conflict.into());
          }
          let current = items::Entity::find_by_id(&request.memory_id)
            .one(&transaction)
            .await?
            .ok_or(MemoryError::NotFound)?;
          if current.state == "deleted" {
            return Err(MemoryError::Conflict.into());
          }
          let previous = revisions::Entity::find_by_id((request.memory_id, request.revision))
            .one(&transaction)
            .await?
            .ok_or(MemoryError::NotFound)?;
          return Ok(serde_json::from_str(&previous.record)?);
        }
      }
      if mutation.item.state == MemoryState::Active
        && mutation
          .item
          .expires_at_ms
          .is_some_and(|expiry| expiry <= agent_core::now_ms())
      {
        return Err(MemoryError::Invalid.into());
      }
      validate_source(&transaction, &mutation).await?;
      let item = &mutation.item;
      let current = items::Entity::find_by_id(&item.id)
        .one(&transaction)
        .await?;
      match current {
        Some(row) => {
          let previous: MemoryItem = serde_json::from_str(&row.record)?;
          if row.state == "deleted" || u64::try_from(row.revision)? != mutation.expected_revision {
            return Err(MemoryError::Conflict.into());
          }
          if (matches!(
            previous.state,
            MemoryState::Rejected | MemoryState::Superseded
          ) && item.state == MemoryState::Active)
            || (matches!(mutation.source, MemorySource::Session { .. })
              && item.state == MemoryState::Proposed
              && previous.state != MemoryState::Proposed)
          {
            return Err(MemoryError::Conflict.into());
          }
          if item.scope != previous.scope
            || item.created_at_ms != previous.created_at_ms
            || item.updated_at_ms < previous.updated_at_ms
          {
            return Err(MemoryError::Invalid.into());
          }
          let changed = items::Entity::update_many()
            .filter(items::Column::Id.eq(&item.id))
            .filter(items::Column::Revision.eq(row.revision))
            .col_expr(items::Column::State, Expr::value(state_name(item.state)))
            .col_expr(
              items::Column::Revision,
              Expr::value(i64::try_from(item.revision)?),
            )
            .col_expr(
              items::Column::Record,
              Expr::value(serde_json::to_string(item)?),
            )
            .exec(&transaction)
            .await?;
          if changed.rows_affected != 1 {
            return Err(MemoryError::Conflict.into());
          }
        }
        None => {
          if mutation.expected_revision != 0 {
            return Err(MemoryError::Conflict.into());
          }
          items::ActiveModel {
            id: Set(item.id.clone()),
            scope_key: Set(scope_key(&item.scope)?),
            state: Set(state_name(item.state).into()),
            revision: Set(i64::try_from(item.revision)?),
            record: Set(serde_json::to_string(item)?),
          }
          .insert(&transaction)
          .await?;
        }
      }
      revisions::ActiveModel {
        memory_id: Set(item.id.clone()),
        revision: Set(i64::try_from(item.revision)?),
        record: Set(serde_json::to_string(item)?),
        source: Set(serde_json::to_string(&mutation.source)?),
      }
      .insert(&transaction)
      .await?;
      if let MemorySource::Explicit { request_id } = &mutation.source {
        requests::ActiveModel {
          request_id: Set(request_id.clone()),
          body_hash: Set(body_hash),
          memory_id: Set(item.id.clone()),
          revision: Set(i64::try_from(item.revision)?),
        }
        .insert(&transaction)
        .await?;
      }
      super::jobs::enqueue(&transaction, item).await?;
      event_tx(
        &transaction,
        &format!("memory:{}", item.id),
        "memory_saved",
        serde_json::json!({"id":item.id,"revision":item.revision,"state":item.state}),
      )
      .await?;
      Ok(item.clone())
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
  pub(super) async fn memory_forget(&self, id: &str, expected_revision: u64) -> Result<MemoryItem> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let row = items::Entity::find_by_id(id)
        .one(&transaction)
        .await?
        .ok_or(MemoryError::NotFound)?;
      if u64::try_from(row.revision)? != expected_revision || row.state == "deleted" {
        return Err(MemoryError::Conflict.into());
      }
      let mut item: MemoryItem = serde_json::from_str(&row.record)?;
      item.revision = expected_revision
        .checked_add(1)
        .ok_or(MemoryError::Invalid)?;
      item.updated_at_ms = agent_core::now_ms();
      item.state = MemoryState::Deleted;
      item.content.clear();
      let changed = items::Entity::update_many()
        .filter(items::Column::Id.eq(id))
        .filter(items::Column::Revision.eq(row.revision))
        .col_expr(
          items::Column::Revision,
          Expr::value(i64::try_from(item.revision)?),
        )
        .col_expr(items::Column::State, Expr::value("deleted"))
        .col_expr(
          items::Column::Record,
          Expr::value(serde_json::to_string(&item)?),
        )
        .exec(&transaction)
        .await?;
      if changed.rows_affected != 1 {
        return Err(MemoryError::Conflict.into());
      }
      // Forget is the privacy exception to immutable history: scrub every version.
      for revision in revisions::Entity::find()
        .filter(revisions::Column::MemoryId.eq(id))
        .all(&transaction)
        .await?
      {
        let mut old: MemoryItem = serde_json::from_str(&revision.record)?;
        old.content.clear();
        old.state = MemoryState::Deleted;
        revisions::Entity::update_many()
          .filter(revisions::Column::MemoryId.eq(id))
          .filter(revisions::Column::Revision.eq(revision.revision))
          .col_expr(
            revisions::Column::Record,
            Expr::value(serde_json::to_string(&old)?),
          )
          .col_expr(revisions::Column::Source, Expr::value(""))
          .exec(&transaction)
          .await?;
      }
      revisions::ActiveModel {
        memory_id: Set(id.into()),
        revision: Set(i64::try_from(item.revision)?),
        record: Set(serde_json::to_string(&item)?),
        source: Set(String::new()),
      }
      .insert(&transaction)
      .await?;
      super::jobs::enqueue(&transaction, &item).await?;
      event_tx(
        &transaction,
        &format!("memory:{id}"),
        "memory_forgotten",
        serde_json::json!({"id":id,"revision":item.revision,"state":item.state}),
      )
      .await?;
      Ok(item)
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
}
