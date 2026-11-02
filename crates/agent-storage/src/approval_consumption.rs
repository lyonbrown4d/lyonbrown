use crate::{
  event_tx,
  schema::{approvals, runs},
  verify_epoch,
};
use agent_core::{Approval, now_ms};
use anyhow::Result;
use sea_orm::{QuerySelect, prelude::*};

/// Shared by approval-only and approval-plus-intent transactions.
pub(crate) async fn consume_approval_tx(
  connection: &impl ConnectionTrait,
  id: &str,
  run_id: &str,
  epoch: Option<u64>,
  tool: &str,
  arguments: &serde_json::Value,
) -> Result<bool> {
  let json: String = approvals::Entity::find()
    .filter(approvals::Column::Id.eq(id))
    .select_only()
    .column(approvals::Column::Record)
    .into_tuple::<String>()
    .one(connection)
    .await?
    .ok_or_else(|| anyhow::anyhow!("approvals record missing"))?;
  let mut value: Approval = serde_json::from_str(&json)?;
  anyhow::ensure!(
    value.run_id == run_id
      && value.tool == tool
      && serde_json::from_str::<serde_json::Value>(&value.arguments_json)? == *arguments,
    "approval action mismatch"
  );
  anyhow::ensure!(value.expires_at_ms > now_ms(), "approval expired");
  if value.status != "approved" {
    return Ok(false);
  }
  let run_status: String = runs::Entity::find()
    .filter(runs::Column::Id.eq(run_id))
    .select_only()
    .column(runs::Column::Status)
    .into_tuple::<String>()
    .one(connection)
    .await?
    .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
  anyhow::ensure!(run_status == "running", "run no longer executing");
  if let Some(epoch) = epoch {
    let session: String = runs::Entity::find()
      .filter(runs::Column::Id.eq(run_id))
      .select_only()
      .column(runs::Column::SessionId)
      .into_tuple::<String>()
      .one(connection)
      .await?
      .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
    verify_epoch(connection, &session, epoch).await?;
  }
  value.status = "consumed".into();
  approvals::Entity::update_many()
    .filter(approvals::Column::Id.eq(id))
    .col_expr(
      approvals::Column::Status,
      Expr::value((&value.status).to_owned()),
    )
    .col_expr(
      approvals::Column::Record,
      Expr::value((serde_json::to_string(&value)?).to_owned()),
    )
    .exec(connection)
    .await
    .map(|result| result.rows_affected)?;
  event_tx(
    connection,
    run_id,
    "tool_authorized",
    serde_json::json!({"approval_id":id,"tool":tool}),
  )
  .await?;
  Ok(true)
}
