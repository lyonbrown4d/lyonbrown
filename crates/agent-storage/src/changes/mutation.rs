use super::{fence, load, record_request, request, update};
use crate::{
  OperationRecord, OperationStatus, Store,
  approval_consumption::consume_approval_tx,
  event_tx,
  operations::begin_operation_tx,
  schema::{operations, runs},
};
use agent_changes::{ChangeSet, ChangeState};
use anyhow::{Context, Result, ensure};
use sea_orm::{QuerySelect, prelude::*};
use serde_json::{Value, json};

pub struct PrepareChange<'a> {
  pub request_id: &'a str,
  pub change_id: &'a str,
  pub expected_revision: u64,
  pub preview_sha256: &'a str,
  pub run_id: &'a str,
  pub owner_epoch: u64,
  pub action_id: &'a str,
  pub arguments: &'a Value,
  pub approval_id: &'a str,
}
/// In-process proof constructed only after exact approval and intent commit.
pub struct PreparedChange {
  pub(super) change: ChangeSet,
}
impl PreparedChange {
  pub fn change(&self) -> &ChangeSet {
    &self.change
  }
}

impl Store {
  pub async fn prepare_change(&self, input: PrepareChange<'_>) -> Result<PreparedChange> {
    uuid::Uuid::parse_str(input.request_id)?;
    uuid::Uuid::parse_str(input.action_id)?;
    let exact = json!({"request_id":input.request_id,"change_id":input.change_id,
      "expected_revision":input.expected_revision,"preview_sha256":input.preview_sha256,
      "run_id":input.run_id,"owner_epoch":input.owner_epoch,"action_id":input.action_id});
    ensure!(
      *input.arguments == exact,
      "change approval does not bind the exact preview"
    );
    let body = agent_changes::digest(&serde_json::to_vec(&exact)?);
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      fence(&transaction, input.run_id, input.owner_epoch).await?;
      ensure!(
        request(&transaction, input.request_id, &body)
          .await?
          .is_none(),
        "change dispatch already recorded; reconcile without replay"
      );
      let mut change = load(&transaction, input.change_id).await?;
      agent_changes::validate(&change)?;
      ensure!(
        change.run_id == input.run_id
          && change.owner_epoch == input.owner_epoch
          && change.revision == input.expected_revision
          && change.state == ChangeState::Preview
          && change.preview_sha256 == input.preview_sha256,
        "change preview or ownership conflict"
      );
      ensure!(
        consume_approval_tx(
          &transaction,
          input.approval_id,
          input.run_id,
          Some(input.owner_epoch),
          "patch_apply",
          &exact
        )
        .await?,
        "change was not approved"
      );
      begin_operation_tx(
        &transaction,
        input.run_id,
        input.owner_epoch,
        input.action_id,
        "patch_apply",
        json!({"argument_count":change.files.len()}),
      )
      .await?;
      change.state = ChangeState::Prepared;
      change.action_id = Some(input.action_id.into());
      change.revision = change
        .revision
        .checked_add(1)
        .context("change revision overflow")?;
      update(&transaction, &change, input.expected_revision).await?;
      record_request(
        &transaction,
        input.request_id,
        &body,
        serde_json::to_string(&change)?,
      )
      .await?;
      Ok(change)
    }
    .await;
    match result {
      Ok(change) => {
        transaction.commit().await?;
        Ok(PreparedChange { change })
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }

  /// Persist the actual host receipt even when cancellation was requested during I/O.
  pub async fn finish_change(
    &self,
    proof: PreparedChange,
    written_paths: Vec<String>,
    complete: bool,
  ) -> Result<ChangeSet> {
    let expected = proof.change.revision;
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let mut change = load(&transaction, &proof.change.id).await?;
      ensure!(
        change.revision == expected
          && change.state == ChangeState::Prepared
          && change.action_id == proof.change.action_id,
        "change receipt conflict"
      );
      // Completion checks epoch but permits a concurrent cancel_requested run.
      let run = runs::Entity::find_by_id(&change.run_id)
        .one(&transaction)
        .await?
        .context("change run missing")?;
      crate::verify_epoch(&transaction, &run.session_id, change.owner_epoch).await?;
      ensure!(
        written_paths.len() <= change.files.len()
          && written_paths
            .iter()
            .zip(&change.files)
            .all(|(path, file)| path == &file.path)
          && (!complete || written_paths.len() == change.files.len()),
        "invalid host change receipt"
      );
      let action = change
        .action_id
        .as_deref()
        .context("change intent missing")?;
      let encoded: String = operations::Entity::find_by_id(action)
        .select_only()
        .column(operations::Column::Record)
        .into_tuple()
        .one(&transaction)
        .await?
        .context("change operation missing")?;
      let mut operation: OperationRecord = serde_json::from_str(&encoded)?;
      ensure!(
        operation.kind == "patch_apply"
          && operation.run_id == change.run_id
          && operation.owner_epoch == change.owner_epoch
          && operation.status == OperationStatus::Intent,
        "change operation conflict"
      );
      operation.status = if complete {
        OperationStatus::Completed
      } else {
        OperationStatus::Uncertain
      };
      operation.finished_at_ms = Some(agent_core::now_ms());
      operation.result_summary = Some(json!({"argument_count":written_paths.len()}));
      ensure!(
        operations::Entity::update_many()
          .filter(operations::Column::ActionId.eq(action))
          .filter(operations::Column::Status.eq("intent"))
          .col_expr(
            operations::Column::Status,
            Expr::value(operation.status.as_str())
          )
          .col_expr(
            operations::Column::Record,
            Expr::value(serde_json::to_string(&operation)?)
          )
          .exec(&transaction)
          .await?
          .rows_affected
          == 1,
        "change operation receipt conflict"
      );
      let applied_event = event_tx(
        &transaction,
        &change.run_id,
        "operation_finished",
        json!({"action_id":action,"status":operation.status}),
      )
      .await?;
      change.applied_event_cursor = complete.then_some(applied_event.cursor);
      change.written_paths = written_paths;
      change.state = if complete {
        ChangeState::Applied
      } else {
        ChangeState::Uncertain
      };
      change.revision = expected
        .checked_add(1)
        .context("change revision overflow")?;
      update(&transaction, &change, expected).await?;
      Ok(change)
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
