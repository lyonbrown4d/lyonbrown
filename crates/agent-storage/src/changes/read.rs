use super::entities::sets;
use crate::{Store, schema::runs};
use anyhow::Result;
use sea_orm::{QueryFilter, QueryOrder, QuerySelect, QueryTrait, prelude::*};
impl Store {
  pub async fn session_changes(
    &self,
    session_id: &str,
    after: Option<&str>,
    limit: u32,
  ) -> Result<Vec<agent_changes::ChangeSet>> {
    let run_ids = runs::Entity::find()
      .select_only()
      .column(runs::Column::Id)
      .filter(runs::Column::SessionId.eq(session_id))
      .into_query();
    let mut query = sets::Entity::find().filter(sets::Column::RunId.in_subquery(run_ids));
    if let Some(after) = after {
      query = query.filter(sets::Column::Id.gt(after));
    }
    query
      .order_by_asc(sets::Column::Id)
      .limit(u64::from(limit.clamp(1, 101)))
      .all(self.connection())
      .await?
      .into_iter()
      .map(|row| Ok(serde_json::from_str(&row.record)?))
      .collect()
  }
}
