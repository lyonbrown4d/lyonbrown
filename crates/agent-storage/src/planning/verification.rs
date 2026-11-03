//! Model-selected operation IDs are evidence references, never caller-supplied outcomes.
use crate::{OperationStatus, Store, schema::events};
use agent_planning::{PlanContext, PlanError, VerificationRecord};
use anyhow::{Context, Result, ensure};
use sea_orm::{QueryFilter, QueryOrder, QuerySelect, prelude::*};
use uuid::Uuid;
impl Store {
  pub async fn plan_verify_operation(
    &self,
    context: &PlanContext,
    request_id: Uuid,
    plan_id: Uuid,
    expected_revision: u64,
    step_id: Uuid,
    operation_id: &str,
  ) -> Result<agent_planning::Plan> {
    let plan = self
      .plan_get(&context.session_id, plan_id)
      .await?
      .ok_or(PlanError::NotFound)?;
    ensure!(plan.run_id == context.run_id, PlanError::NotFound);
    let step = plan
      .steps
      .iter()
      .find(|step| step.definition.id == step_id)
      .ok_or(PlanError::NotFound)?;
    let operation = self
      .operation(operation_id)
      .await?
      .context("host verification operation missing")?;
    ensure!(
      operation.run_id == context.run_id
        && operation.owner_epoch == context.owner_epoch
        && operation.kind == "execute_command"
        && matches!(
          operation.status,
          OperationStatus::Completed | OperationStatus::Failed
        ),
      "verification requires a terminal command in the same owned run"
    );
    // Bounded durable event ordering prevents reusing a command from before work began.
    let mut rows = events::Entity::find()
      .filter(events::Column::RunId.eq(&context.run_id))
      .order_by_desc(events::Column::Cursor)
      .limit(4096)
      .all(self.connection())
      .await?;
    rows.reverse();
    let events = rows
      .into_iter()
      .map(|row| serde_json::from_str::<agent_core::Event>(&row.record))
      .collect::<std::result::Result<Vec<_>, _>>()?;
    let plan_id_text = plan_id.to_string();
    let step_id_text = step_id.to_string();
    let mut started = None;
    let mut command = None;
    for event in events {
      let payload: serde_json::Value = serde_json::from_str(&event.payload_json)?;
      if event.kind == "plan_updated"
        && payload.get("plan_id").and_then(|value| value.as_str()) == Some(plan_id_text.as_str())
      {
        let change = &payload["detail"]["change"];
        if change["step_id"].as_str() == Some(step_id_text.as_str())
          && change["state"].as_str() == Some("running")
        {
          started = Some(event.cursor);
        }
      }
      if event.kind == "operation_intent" && payload["action_id"].as_str() == Some(operation_id) {
        command = Some(event.cursor);
      }
    }
    ensure!(
      started
        .zip(command)
        .is_some_and(|(start, command)| command > start),
      "command must follow the step start within the latest 4096 run events"
    );
    let exit = operation
      .result_summary
      .as_ref()
      .and_then(|summary| summary.get("exit_code"))
      .and_then(|value| value.as_i64());
    let record = VerificationRecord {
      id: request_id,
      step_id,
      evidence_ref: format!("operation:{operation_id}"),
      acceptance: step.definition.acceptance.clone(),
      passed: operation.status == OperationStatus::Completed && exit == Some(0),
      summary: format!(
        "Host command receipt: {} / exit {exit:?}",
        operation.status.as_str()
      ),
    };
    self
      .plan_record_verification(context, request_id, plan_id, expected_revision, record)
      .await
  }
}
