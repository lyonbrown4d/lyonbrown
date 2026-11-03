use crate::{OperationRecord, OperationStatus, event_tx, schema::operations};
use anyhow::{Context, Result, ensure};
use sea_orm::{QuerySelect, prelude::*};
use serde_json::json;

/// May run before or after generic operation recovery, within the changes txn.
pub(super) async fn recover_intent(connection: &impl ConnectionTrait, action: &str) -> Result<()> {
  let encoded: String = operations::Entity::find_by_id(action)
    .select_only()
    .column(operations::Column::Record)
    .into_tuple()
    .one(connection)
    .await?
    .context("change operation missing")?;
  let mut operation: OperationRecord = serde_json::from_str(&encoded)?;
  ensure!(
    operation.kind == "patch_apply",
    "change operation binding mismatch"
  );
  if operation.status == OperationStatus::Uncertain {
    return Ok(());
  }
  ensure!(
    operation.status == OperationStatus::Intent,
    "prepared change has contradictory terminal operation"
  );
  operation.status = OperationStatus::Uncertain;
  operation.finished_at_ms = Some(agent_core::now_ms());
  operation.result_summary = Some(json!({"reason":"daemon_restarted"}));
  ensure!(
    operations::Entity::update_many()
      .filter(operations::Column::ActionId.eq(action))
      .filter(operations::Column::Status.eq("intent"))
      .col_expr(operations::Column::Status, Expr::value("uncertain"))
      .col_expr(
        operations::Column::Record,
        Expr::value(serde_json::to_string(&operation)?)
      )
      .exec(connection)
      .await?
      .rows_affected
      == 1,
    "change recovery conflict"
  );
  event_tx(
    connection,
    &operation.run_id,
    "operation_finished",
    json!({"action_id":action,"status":"uncertain"}),
  )
  .await?;
  Ok(())
}
