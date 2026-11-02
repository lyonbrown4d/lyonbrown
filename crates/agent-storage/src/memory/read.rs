use super::{entities::items, scope_key, state_name};
use crate::Store;
use anyhow::Result;
use memory_core::{MemoryError, MemoryItem, MemoryPage, MemoryScope, MemoryState};
use sea_orm::{QueryOrder, QuerySelect, prelude::*};
impl Store {
  pub(super) async fn memory_get(&self, id: &str) -> Result<Option<MemoryItem>> {
    items::Entity::find_by_id(id)
      .one(self.connection())
      .await?
      .map(|row| Ok(serde_json::from_str(&row.record)?))
      .transpose()
  }
  pub(super) async fn memory_list(
    &self,
    scopes: &[MemoryScope],
    states: &[MemoryState],
    after: Option<&str>,
    limit: u32,
  ) -> Result<MemoryPage> {
    if !(1..=100).contains(&limit) || scopes.len() > 100 || states.len() > 5 {
      return Err(MemoryError::Invalid.into());
    }
    if scopes.is_empty() || states.is_empty() {
      return Ok(MemoryPage {
        items: vec![],
        next_cursor: None,
      });
    }
    let keys = scopes.iter().map(scope_key).collect::<Result<Vec<_>>>()?;
    let mut query = items::Entity::find()
      .filter(items::Column::ScopeKey.is_in(keys))
      .filter(items::Column::State.is_in(states.iter().copied().map(state_name)));
    if let Some(after) = after {
      query = query.filter(items::Column::Id.gt(after));
    }
    let rows = query
      .order_by_asc(items::Column::Id)
      .limit(u64::from(limit) + 1)
      .all(self.connection())
      .await?;
    let mut values = rows
      .into_iter()
      .map(|row| Ok(serde_json::from_str::<MemoryItem>(&row.record)?))
      .collect::<Result<Vec<_>>>()?;
    let more = values.len() > limit as usize;
    values.truncate(limit as usize);
    let next_cursor = if more {
      values.last().map(|item| item.id.clone())
    } else {
      None
    };
    Ok(MemoryPage {
      items: values,
      next_cursor,
    })
  }
}
