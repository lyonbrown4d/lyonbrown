use super::entities::sets;
use crate::schema::runs;
use anyhow::{Result, ensure};
use sea_orm::{QuerySelect, QueryTrait, prelude::*};
/// A detached native writer may outlive its RPC future. Exclude new run or
/// user-change admission until its durable checkpoint receipt is reconciled.
pub(crate) async fn ensure_no_prepared_change(
  connection: &impl ConnectionTrait,
  session: &str,
) -> Result<()> {
  let ids = runs::Entity::find()
    .filter(runs::Column::SessionId.eq(session))
    .select_only()
    .column(runs::Column::Id)
    .into_query();
  let active = sets::Entity::find()
    .filter(sets::Column::RunId.in_subquery(ids))
    .filter(sets::Column::State.eq("prepared"))
    .count(connection)
    .await?;
  ensure!(
    active == 0,
    "a prepared filesystem change requires a receipt before admitting new work"
  );
  Ok(())
}
