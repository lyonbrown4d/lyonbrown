//! Durable plans share the Session event/outbox transaction and owner fence.
mod entities;
mod verification;
mod write;
use crate::Store;
use agent_planning::{Plan, PlanPage};
use anyhow::Result;
use entities::plans;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use uuid::Uuid;

impl Store {
  pub async fn plan_get(&self, session_id: &str, plan_id: Uuid) -> Result<Option<Plan>> {
    plans::Entity::find_by_id(plan_id.to_string())
      .filter(plans::Column::SessionId.eq(session_id))
      .one(self.connection())
      .await?
      .map(|row| Ok(serde_json::from_str(&row.record)?))
      .transpose()
  }
  /// Stable UUID ordering; limit is bounded to 100. A cursor is the final returned UUID.
  pub async fn plan_list(
    &self,
    session_id: &str,
    run_id: Option<&str>,
    after: Option<Uuid>,
    limit: u32,
  ) -> Result<PlanPage> {
    let limit = limit.clamp(1, 100) as usize;
    let mut query = plans::Entity::find().filter(plans::Column::SessionId.eq(session_id));
    if let Some(run) = run_id {
      query = query.filter(plans::Column::RunId.eq(run));
    }
    if let Some(after) = after {
      query = query.filter(plans::Column::Id.gt(after.to_string()));
    }
    let mut rows = query
      .order_by_asc(plans::Column::Id)
      .limit((limit + 1) as u64)
      .all(self.connection())
      .await?;
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let plans = rows
      .into_iter()
      .map(|row| Ok(serde_json::from_str::<Plan>(&row.record)?))
      .collect::<Result<Vec<_>>>()?;
    let next_after = if has_more {
      plans.last().map(|plan| plan.id)
    } else {
      None
    };
    Ok(PlanPage { plans, next_after })
  }
}
