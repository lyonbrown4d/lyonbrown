//! An exact approved preparation becomes durable before native input dispatch.
use super::router::PinnedHost;
use crate::Daemon;
use agent_computer::ActionOutcome;
use agent_computer_runtime::{Operation, Preparation, Reply};
use agent_runtime::RuntimeError;
use agent_storage::{OperationAuthorization, OperationStatus, Store};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub(super) struct CommittedAction {
  guard: ReceiptGuard,
  proof: OperationAuthorization,
  host: PinnedHost,
  preparation: Preparation,
  operation: Operation,
}
impl CommittedAction {
  pub(super) fn new(
    store: Store,
    proof: OperationAuthorization,
    host: PinnedHost,
    preparation: Preparation,
    operation: Operation,
  ) -> Self {
    Self {
      guard: ReceiptGuard {
        store,
        run: proof.run_id().into(),
        epoch: proof.owner_epoch(),
        action: proof.action_id().into(),
        finished: false,
      },
      proof,
      host,
      preparation,
      operation,
    }
  }

  pub(super) async fn execute(
    mut self,
    daemon: &Daemon,
    cancellation: CancellationToken,
  ) -> Result<Value> {
    let outcome = async {
      ensure!(
        self.proof.run_id() == self.preparation.binding.run_id
          && self.proof.owner_epoch() == self.preparation.binding.owner_epoch
          && self.proof.action_id() == self.preparation.request.action_id.to_string(),
        "computer committed binding changed"
      );
      let Operation::Execute {
        preparation_id,
        action_id,
        content_digest,
        approval_id,
        ..
      } = &self.operation
      else {
        anyhow::bail!("computer mutation must execute a preparation");
      };
      ensure!(
        *preparation_id == self.preparation.preparation_id
          && *action_id == self.preparation.request.action_id
          && *content_digest == self.preparation.content_digest
          && approval_id.to_string() == self.proof.approval_id(),
        "computer committed action changed"
      );
      daemon.validate_computer_run(&self.preparation.binding)?;
      let reply = daemon
        .computer
        .request(
          &self.host,
          self.preparation.binding.clone(),
          self.operation.clone(),
          cancellation,
        )
        .await?;
      // The engine returns this typed error from preparation refresh before it
      // can enter native input. A changed page needs a new observation/approval.
      let reply = match reply {
        Err(agent_computer_runtime::RuntimeError::Desktop(
          agent_computer::DesktopError::StaleSnapshot,
        )) => {
          return Ok(ActionOutcome::Rejected(
            agent_computer::DesktopError::StaleSnapshot,
          ));
        }
        result => result?,
      };
      let Reply::Executed(result) = reply else {
        anyhow::bail!("invalid computer execution receipt")
      };
      ensure!(
        result.binding == self.preparation.binding
          && result.preparation_id == self.preparation.preparation_id
          && result.original_request == self.preparation.request
          && result.content_digest == self.preparation.content_digest
          && result.receipt.action_id == self.preparation.request.action_id
          && result.receipt.approval_id.to_string() == self.proof.approval_id(),
        "computer receipt does not match approved preparation"
      );
      Ok::<_, anyhow::Error>(result.receipt.outcome)
    }
    .await;
    let status = match &outcome {
      Ok(ActionOutcome::Completed) => OperationStatus::Completed,
      Ok(ActionOutcome::Rejected(_)) => OperationStatus::Failed,
      _ => OperationStatus::Uncertain,
    };
    self
      .guard
      .store
      .finish_operation(
        &self.guard.run,
        self.guard.epoch,
        &self.guard.action,
        status,
        json!({"status":status.as_str()}),
      )
      .map_err(|_| {
        RuntimeError::Uncertain("computer receipt could not be persisted; do not replay".into())
      })?;
    self.guard.finished = true;
    daemon.events.notify_waiters();
    match status {
      OperationStatus::Completed => Ok(json!({"action_id":self.guard.action,"status":"completed"})),
      OperationStatus::Failed => anyhow::bail!("native input rejected before dispatch"),
      _ => Err(
        RuntimeError::Uncertain(
          "computer input outcome requires verification; it was not replayed".into(),
        )
        .into(),
      ),
    }
  }
}

struct ReceiptGuard {
  store: Store,
  run: String,
  epoch: u64,
  action: String,
  finished: bool,
}
impl Drop for ReceiptGuard {
  fn drop(&mut self) {
    if !self.finished
      && self
        .store
        .finish_operation(
          &self.run,
          self.epoch,
          &self.action,
          OperationStatus::Uncertain,
          json!({"status":"uncertain"}),
        )
        .is_err()
    {
      tracing::warn!(action_id=%self.action, "computer intent remains unresolved after caller stopped");
    }
  }
}
