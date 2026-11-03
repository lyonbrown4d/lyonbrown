//! Explicit authenticated user approval outside a running model loop.
use super::{
  PreparedChange,
  entities::{sets, user_approvals},
  load, record_request, request, update,
};
use crate::{
  OperationRecord, OperationStatus, Store, event_tx,
  schema::{operations, runs},
  verify_epoch,
};
use agent_changes::{ChangeSet, ChangeState};
use anyhow::{Context, Result, ensure};
use sea_orm::{Set, prelude::*};
use serde_json::json;

pub struct UserPrepareChange<'a> {
  pub session_id: &'a str,
  pub owner_epoch: u64,
  pub request_id: &'a str,
  pub change_id: &'a str,
  pub expected_revision: u64,
  pub preview_sha256: &'a str,
  pub action_id: &'a str,
}
async fn idle_fence(
  connection: &impl ConnectionTrait,
  session: &str,
  epoch: u64,
  run: &str,
) -> Result<()> {
  verify_epoch(connection, session, epoch).await?;
  super::ensure_no_prepared_change(connection, session).await?;
  crate::context_maintenance::ensure_idle(connection, session).await?;
  let original = runs::Entity::find_by_id(run)
    .one(connection)
    .await?
    .context("change audit run missing")?;
  ensure!(
    original.session_id == session,
    "change belongs to another session"
  );
  let active = runs::Entity::find()
    .filter(runs::Column::SessionId.eq(session))
    .filter(runs::Column::Status.is_in([
      "accepted",
      "running",
      "awaiting_approval",
      "cancel_requested",
      "uncertain",
    ]))
    .count(connection)
    .await?;
  ensure!(
    active == 0,
    "finish or reconcile all session runs before an explicit user change"
  );
  Ok(())
}
impl Store {
  pub async fn save_user_change_preview(
    &self,
    session: &str,
    epoch: u64,
    request_id: &str,
    change: ChangeSet,
  ) -> Result<ChangeSet> {
    agent_changes::validate(&change)?;
    ensure!(
      change.owner_epoch == epoch
        && change.state == ChangeState::Preview
        && change.revision == 1
        && change.action_id.is_none()
        && change.written_paths.is_empty()
        && change.applied_event_cursor.is_none(),
      "invalid user change preview"
    );
    let body = agent_changes::digest(&serde_json::to_vec(&(session, &change))?);
    let transaction = self.begin_write().await?;
    let result:Result<_>=async {
      idle_fence(&transaction,session,epoch,&change.run_id).await?;
      if let Some(record)=request(&transaction,request_id,&body).await? {return Ok(serde_json::from_str(&record)?);}
      sets::ActiveModel {id:Set(change.id.clone()),run_id:Set(change.run_id.clone()),revision:Set(1),state:Set("preview".into()),record:Set(serde_json::to_string(&change)?)}.insert(&transaction).await?;
      record_request(&transaction,request_id,&body,serde_json::to_string(&change)?).await?;
      event_tx(&transaction,&change.run_id,"user_change_previewed",json!({"change_id":change.id,"preview_sha256":change.preview_sha256,"revision":change.revision})).await?;
      Ok(change)
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
  /// The caller is an authenticated explicit user RPC after presenting the exact
  /// preview. Model tools cannot invoke this separate authorization boundary.
  pub async fn prepare_user_change(&self, input: UserPrepareChange<'_>) -> Result<PreparedChange> {
    uuid::Uuid::parse_str(input.request_id)?;
    uuid::Uuid::parse_str(input.action_id)?;
    let exact = json!({"session_id":input.session_id,"owner_epoch":input.owner_epoch,"request_id":input.request_id,
      "change_id":input.change_id,"expected_revision":input.expected_revision,"preview_sha256":input.preview_sha256,"action_id":input.action_id});
    let body = agent_changes::digest(&serde_json::to_vec(&exact)?);
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let mut change = load(&transaction, input.change_id).await?;
      idle_fence(
        &transaction,
        input.session_id,
        input.owner_epoch,
        &change.run_id,
      )
      .await?;
      ensure!(
        request(&transaction, input.request_id, &body)
          .await?
          .is_none(),
        "user change already dispatched; inspect without replay"
      );
      agent_changes::validate(&change)?;
      ensure!(
        change.owner_epoch == input.owner_epoch
          && change.revision == input.expected_revision
          && change.state == ChangeState::Preview
          && change.preview_sha256 == input.preview_sha256,
        "reviewed user change conflicts"
      );
      ensure!(
        operations::Entity::find_by_id(input.action_id)
          .one(&transaction)
          .await?
          .is_none(),
        "user action already recorded; replay is forbidden"
      );
      user_approvals::ActiveModel {
        action_id: Set(input.action_id.into()),
        request_id: Set(input.request_id.into()),
        session_id: Set(input.session_id.into()),
        owner_epoch: Set(i64::try_from(input.owner_epoch)?),
        body_hash: Set(body.clone()),
        record: Set(serde_json::to_string(&exact)?),
      }
      .insert(&transaction)
      .await?;
      event_tx(
        &transaction,
        &change.run_id,
        "user_change_authorized",
        exact,
      )
      .await?;
      let operation = OperationRecord {
        action_id: input.action_id.into(),
        run_id: change.run_id.clone(),
        owner_epoch: input.owner_epoch,
        kind: "patch_apply".into(),
        status: OperationStatus::Intent,
        intent_summary: json!({"argument_count":change.files.len()}),
        result_summary: None,
        created_at_ms: agent_core::now_ms(),
        finished_at_ms: None,
      };
      operations::ActiveModel {
        action_id: Set(operation.action_id.clone()),
        run_id: Set(change.run_id.clone()),
        owner_epoch: Set(i64::try_from(input.owner_epoch)?),
        status: Set("intent".into()),
        record: Set(serde_json::to_string(&operation)?),
      }
      .insert(&transaction)
      .await?;
      event_tx(
        &transaction,
        &change.run_id,
        "operation_intent",
        json!({"action_id":input.action_id,"kind":"patch_apply"}),
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
}
