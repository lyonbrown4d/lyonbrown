use crate::{
  Store,
  approvals::consume_approval_tx,
  operations::{begin_operation_tx, validate_intent},
};
use anyhow::{Result, ensure};
use serde_json::Value;

/// Exact approval binding and bounded metadata for one durable operation intent.
pub struct ApprovedOperation<'a> {
  pub run_id: &'a str,
  pub owner_epoch: u64,
  pub action_id: &'a str,
  pub tool: &'a str,
  pub arguments: &'a Value,
  pub summary: Value,
  pub approval_id: &'a str,
}

/// In-process proof that approval consumption and intent committed together.
/// It cannot be constructed by callers or restored from a wire representation.
#[derive(Debug)]
pub struct OperationAuthorization {
  action_id: String,
  approval_id: String,
  run_id: String,
  owner_epoch: u64,
}

impl OperationAuthorization {
  pub fn action_id(&self) -> &str {
    &self.action_id
  }

  pub fn approval_id(&self) -> &str {
    &self.approval_id
  }

  pub fn run_id(&self) -> &str {
    &self.run_id
  }

  pub fn owner_epoch(&self) -> u64 {
    self.owner_epoch
  }
}

impl Store {
  /// Consume a matching approval and commit intent, events and outbox atomically.
  /// Callers must receive this proof before dispatching the native operation.
  pub fn begin_approved_operation(
    &self,
    input: ApprovedOperation<'_>,
  ) -> Result<OperationAuthorization> {
    validate_intent(input.action_id, input.tool, &input.summary)?;
    ensure!(
      input.arguments.get("action_id").and_then(Value::as_str) == Some(input.action_id),
      "operation identifier does not match approved action"
    );
    self.lock()?.immediate_transaction(|connection| {
      ensure!(
        consume_approval_tx(
          connection,
          input.approval_id,
          input.run_id,
          Some(input.owner_epoch),
          input.tool,
          input.arguments,
        )?,
        "operation approval is not approved"
      );
      begin_operation_tx(
        connection,
        input.run_id,
        input.owner_epoch,
        input.action_id,
        input.tool,
        input.summary,
      )
    })?;
    // Construct only after SQLite confirms the commit, never within the closure.
    Ok(OperationAuthorization {
      action_id: input.action_id.into(),
      approval_id: input.approval_id.into(),
      run_id: input.run_id.into(),
      owner_epoch: input.owner_epoch,
    })
  }
}
