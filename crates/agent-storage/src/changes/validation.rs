use super::{
  entities::{sets, validations},
  fence, load, record_request, request,
};
use crate::{
  OperationRecord, OperationStatus, Store, event_tx,
  schema::{events, operations},
};
use agent_changes::{ChangeState, ValidationRecord};
use anyhow::{Context, Result, ensure};
use sea_orm::{QueryOrder, QuerySelect, Set, prelude::*};
use serde_json::json;

async fn operation(connection: &impl ConnectionTrait, action: &str) -> Result<OperationRecord> {
  let encoded: String = operations::Entity::find_by_id(action)
    .select_only()
    .column(operations::Column::Record)
    .into_tuple()
    .one(connection)
    .await?
    .context("host command operation missing")?;
  Ok(serde_json::from_str(&encoded)?)
}
impl Store {
  /// Status is derived only from a persisted host command receipt after the change.
  pub async fn record_change_validation(
    &self,
    request_id: &str,
    change_id: &str,
    run_id: &str,
    epoch: u64,
    expected_revision: u64,
    operation_id: &str,
  ) -> Result<ValidationRecord> {
    let body = agent_changes::digest(&serde_json::to_vec(&(
      change_id,
      run_id,
      epoch,
      expected_revision,
      operation_id,
    ))?);
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      fence(&transaction, run_id, epoch).await?;
      if let Some(record) = request(&transaction, request_id, &body).await? { return Ok(serde_json::from_str(&record)?); }
      let change = load(&transaction, change_id).await?;
      ensure!(change.run_id == run_id && change.owner_epoch == epoch && change.state == ChangeState::Applied && change.revision == expected_revision, "validation checkpoint conflict");
      let later_changes = sets::Entity::find().filter(sets::Column::RunId.eq(run_id))
        .filter(sets::Column::State.eq("applied")).all(&transaction).await?;
      for row in later_changes {
        let later: agent_changes::ChangeSet = serde_json::from_str(&row.record)?;
        ensure!(!(later.applied_event_cursor > change.applied_event_cursor && later.files.iter()
          .any(|file|change.files.iter().any(|original|original.path==file.path))),
          "a later applied change overlaps this historical checkpoint");
      }
      let applied = operation(&transaction, change.action_id.as_deref().context("change operation missing")?).await?;
      let command = operation(&transaction, operation_id).await?;
      ensure!(command.run_id == run_id && command.owner_epoch == epoch && command.kind == "execute_command"
        && matches!(command.status, OperationStatus::Completed | OperationStatus::Failed)
        && command.created_at_ms >= applied.finished_at_ms.context("change not completed")?, "validation requires a later terminal host command in the same run");
      // Millisecond timestamps alone cannot order a command and an apply in the
      // same tick. Require its real host intent event beyond the apply receipt.
      let cursor = change.applied_event_cursor.context("change event fence missing")?;
      let later = events::Entity::find().filter(events::Column::RunId.eq(run_id))
        .filter(events::Column::Cursor.gt(i64::try_from(cursor)?))
        .order_by_asc(events::Column::Cursor).limit(4096).all(&transaction).await?;
      let mut command_after_apply = false;
      for row in later {
        let event: agent_core::Event = serde_json::from_str(&row.record)?;
        if event.kind == "operation_intent" {
          let payload: serde_json::Value = serde_json::from_str(&event.payload_json)?;
          command_after_apply |= payload.get("action_id").and_then(serde_json::Value::as_str) == Some(operation_id);
        }
      }
      ensure!(command_after_apply, "host command intent must follow apply receipt within the 4096-event validation window");
      let summary = command.result_summary.as_ref().context("command receipt missing")?;
      let exit_code = summary.get("exit_code").and_then(serde_json::Value::as_i64);
      let record = ValidationRecord {
        id: request_id.into(), change_id: change_id.into(), change_revision: expected_revision,
        change_sha256: change.preview_sha256, operation_id: operation_id.into(),
        passed: command.status == OperationStatus::Completed && exit_code == Some(0), verifies_current_workspace:false, exit_code,
      };
      validations::ActiveModel { id: Set(record.id.clone()), change_id: Set(change_id.into()), record: Set(serde_json::to_string(&record)?) }.insert(&transaction).await?;
      record_request(&transaction, request_id, &body, serde_json::to_string(&record)?).await?;
      event_tx(&transaction, run_id, "change_validation_recorded", json!({"validation_id":record.id,"change_id":change_id,"operation_id":operation_id,"passed":record.passed,"verifies_current_workspace":false})).await?;
      Ok(record)
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
  /// Bounded evidence read for model tools; the extra row signals truncation.
  pub async fn recent_change_validations(
    &self,
    change_id: &str,
    limit: u32,
  ) -> Result<Vec<ValidationRecord>> {
    validations::Entity::find()
      .filter(validations::Column::ChangeId.eq(change_id))
      .order_by_desc(validations::Column::Id)
      .limit(u64::from(limit.clamp(1, 65)))
      .all(self.connection())
      .await?
      .into_iter()
      .map(|row| Ok(serde_json::from_str(&row.record)?))
      .collect()
  }
  pub async fn change_validations(&self, change_id: &str) -> Result<Vec<ValidationRecord>> {
    validations::Entity::find()
      .filter(validations::Column::ChangeId.eq(change_id))
      .all(self.connection())
      .await?
      .into_iter()
      .map(|row| Ok(serde_json::from_str(&row.record)?))
      .collect()
  }
}
