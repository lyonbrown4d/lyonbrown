use crate::{
  Store, decode, event_tx,
  schema::{approvals, runs},
  verify_epoch,
};
use agent_core::{Approval, Run, new_id, now_ms};
use anyhow::Result;
use diesel::prelude::*;

pub(crate) fn close_pending(connection: &mut SqliteConnection, run: &str) -> Result<()> {
  let pending: Vec<String> = approvals::table
    .filter(approvals::run_id.eq(run))
    .filter(approvals::status.eq("pending"))
    .select(approvals::record)
    .load(connection)?;
  for record in pending {
    let mut approval: Approval = serde_json::from_str(&record)?;
    approval.status = "cancelled".into();
    diesel::update(approvals::table.find(&approval.id))
      .set((
        approvals::status.eq(&approval.status),
        approvals::record.eq(serde_json::to_string(&approval)?),
      ))
      .execute(connection)?;
    event_tx(
      connection,
      run,
      "approval_closed",
      serde_json::json!({"approval_id":approval.id,"status":"cancelled"}),
    )?;
  }
  Ok(())
}

impl Store {
  pub fn request_approval(
    &self,
    run_id: &str,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<Approval> {
    self.request_approval_inner(run_id, None, tool, arguments)
  }
  pub fn request_approval_owned(
    &self,
    run_id: &str,
    epoch: u64,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<Approval> {
    self.request_approval_inner(run_id, Some(epoch), tool, arguments)
  }
  fn request_approval_inner(
    &self,
    run_id: &str,
    epoch: Option<u64>,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<Approval> {
    self.lock()?.immediate_transaction(|connection| {
      let value = Approval {
        id: new_id(),
        run_id: run_id.into(),
        tool: tool.into(),
        arguments_json: serde_json::to_string(arguments)?,
        status: "pending".into(),
        expires_at_ms: now_ms() + 300_000,
      };
      let json: String = runs::table
        .find(run_id)
        .select(runs::record)
        .first(connection)?;
      let mut run: Run = serde_json::from_str(&json)?;
      if let Some(epoch) = epoch {
        verify_epoch(connection, &run.session_id, epoch)?;
      }
      anyhow::ensure!(run.status == "running", "run is not executing");
      run.status = "awaiting_approval".into();
      diesel::update(runs::table.find(run_id))
        .set((
          runs::status.eq(&run.status),
          runs::record.eq(serde_json::to_string(&run)?),
        ))
        .execute(connection)?;
      diesel::insert_into(approvals::table)
        .values((
          approvals::id.eq(&value.id),
          approvals::run_id.eq(run_id),
          approvals::status.eq(&value.status),
          approvals::record.eq(serde_json::to_string(&value)?),
        ))
        .execute(connection)?;
      event_tx(
        connection,
        run_id,
        "approval_required",
        serde_json::json!({"approval_id":value.id,"tool":tool,"arguments":arguments}),
      )?;
      Ok(value)
    })
  }
  pub fn approvals(&self, run_id: &str) -> Result<Vec<Approval>> {
    decode(
      approvals::table
        .filter(approvals::run_id.eq(run_id))
        .order(approvals::id.asc())
        .select(approvals::record)
        .load(&mut *self.lock()?)?,
    )
  }
  pub fn approval(&self, id: &str) -> Result<Option<Approval>> {
    let json: Option<String> = approvals::table
      .find(id)
      .select(approvals::record)
      .first(&mut *self.lock()?)
      .optional()?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub fn decide_approval(&self, id: &str, approved: bool) -> Result<Approval> {
    self.lock()?.immediate_transaction(|connection| {
      let json: String = approvals::table
        .find(id)
        .select(approvals::record)
        .first(connection)?;
      let mut value: Approval = serde_json::from_str(&json)?;
      anyhow::ensure!(value.status == "pending", "approval already decided");
      anyhow::ensure!(value.expires_at_ms > now_ms(), "approval expired");
      let json: String = runs::table
        .find(&value.run_id)
        .select(runs::record)
        .first(connection)?;
      let mut run: Run = serde_json::from_str(&json)?;
      anyhow::ensure!(
        run.status == "awaiting_approval",
        "run no longer awaiting approval"
      );
      run.status = "running".into();
      value.status = if approved { "approved" } else { "denied" }.into();
      diesel::update(runs::table.find(&run.id))
        .set((
          runs::status.eq(&run.status),
          runs::record.eq(serde_json::to_string(&run)?),
        ))
        .execute(connection)?;
      diesel::update(approvals::table.find(id))
        .set((
          approvals::status.eq(&value.status),
          approvals::record.eq(serde_json::to_string(&value)?),
        ))
        .execute(connection)?;
      event_tx(
        connection,
        &run.id,
        "approval_decided",
        serde_json::json!({"approval_id":id,"status":value.status}),
      )?;
      Ok(value)
    })
  }
  pub fn consume_approval(
    &self,
    id: &str,
    run_id: &str,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<bool> {
    self.consume_approval_inner(id, run_id, None, tool, arguments)
  }
  pub fn consume_approval_owned(
    &self,
    id: &str,
    run_id: &str,
    epoch: u64,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<bool> {
    self.consume_approval_inner(id, run_id, Some(epoch), tool, arguments)
  }
  fn consume_approval_inner(
    &self,
    id: &str,
    run_id: &str,
    epoch: Option<u64>,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<bool> {
    self.lock()?.immediate_transaction(|connection| {
      consume_approval_tx(connection, id, run_id, epoch, tool, arguments)
    })
  }
}

/// Shared by approval-only and approval-plus-intent transactions.
pub(crate) fn consume_approval_tx(
  connection: &mut SqliteConnection,
  id: &str,
  run_id: &str,
  epoch: Option<u64>,
  tool: &str,
  arguments: &serde_json::Value,
) -> Result<bool> {
  let json: String = approvals::table
    .find(id)
    .select(approvals::record)
    .first(connection)?;
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
  let run_status: String = runs::table
    .find(run_id)
    .select(runs::status)
    .first(connection)?;
  anyhow::ensure!(run_status == "running", "run no longer executing");
  if let Some(epoch) = epoch {
    let session: String = runs::table
      .find(run_id)
      .select(runs::session_id)
      .first(connection)?;
    verify_epoch(connection, &session, epoch)?;
  }
  value.status = "consumed".into();
  diesel::update(approvals::table.find(id))
    .set((
      approvals::status.eq(&value.status),
      approvals::record.eq(serde_json::to_string(&value)?),
    ))
    .execute(connection)?;
  event_tx(
    connection,
    run_id,
    "tool_authorized",
    serde_json::json!({"approval_id":id,"tool":tool}),
  )?;
  Ok(true)
}
