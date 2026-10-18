use crate::{
  Store, approvals::consume_approval_tx, browser_actions::begin_browser_action_tx, schema::runs,
};
use anyhow::{Result, ensure};
use diesel::prelude::*;
use serde_json::Value;

/// Exact model approval binding and redacted metadata for one browser intent.
pub struct ApprovedBrowserAction<'a> {
  pub run_id: &'a str,
  pub owner_epoch: u64,
  pub action_id: &'a str,
  pub tool: &'a str,
  pub arguments: &'a Value,
  pub summary: Value,
  pub approval_id: &'a str,
}

/// In-process proof that exact approval and browser intent committed together.
/// Callers cannot construct, clone, or restore it from a wire representation.
#[derive(Debug)]
pub struct BrowserActionAuthorization {
  session_id: String,
  run_id: String,
  owner_epoch: u64,
  action_id: String,
  approval_id: String,
  host_id: String,
}

impl BrowserActionAuthorization {
  pub fn session_id(&self) -> &str {
    &self.session_id
  }

  pub fn run_id(&self) -> &str {
    &self.run_id
  }

  pub fn owner_epoch(&self) -> u64 {
    self.owner_epoch
  }

  pub fn action_id(&self) -> &str {
    &self.action_id
  }

  pub fn approval_id(&self) -> &str {
    &self.approval_id
  }

  pub fn host_id(&self) -> &str {
    &self.host_id
  }
}

impl Store {
  /// Atomically consume exact model approval and commit browser intent and Outbox.
  /// The authenticated human browser API continues to record its own intents.
  pub fn begin_approved_browser_action(
    &self,
    input: ApprovedBrowserAction<'_>,
  ) -> Result<BrowserActionAuthorization> {
    ensure!(input.tool == "browser_act", "invalid approved browser tool");
    let host_id = input
      .arguments
      .get("host_id")
      .and_then(Value::as_str)
      .filter(|id| !id.is_empty() && id.len() <= 128)
      .ok_or_else(|| anyhow::anyhow!("invalid approved browser host"))?;
    ensure!(
      input.arguments.get("action_id").and_then(Value::as_str) == Some(input.action_id)
        && input.arguments.get("owner_epoch").and_then(Value::as_u64) == Some(input.owner_epoch),
      "browser action or owner epoch does not match approval"
    );
    let session_id = self.lock()?.immediate_transaction(|connection| {
      let session: String = runs::table
        .find(input.run_id)
        .select(runs::session_id)
        .first(connection)?;
      ensure!(
        input.arguments.get("session_id").and_then(Value::as_str) == Some(session.as_str()),
        "browser approval does not belong to the run session"
      );
      ensure!(
        consume_approval_tx(
          connection,
          input.approval_id,
          input.run_id,
          Some(input.owner_epoch),
          input.tool,
          input.arguments,
        )?,
        "browser action approval is not approved"
      );
      begin_browser_action_tx(
        connection,
        &session,
        input.owner_epoch,
        input.action_id,
        "browser_command",
        input.summary,
      )?;
      Ok::<_, anyhow::Error>(session)
    })?;
    // A proof exists only after SQLite confirms the approval/intent commit.
    Ok(BrowserActionAuthorization {
      session_id,
      run_id: input.run_id.into(),
      owner_epoch: input.owner_epoch,
      action_id: input.action_id.into(),
      approval_id: input.approval_id.into(),
      host_id: host_id.into(),
    })
  }
}
