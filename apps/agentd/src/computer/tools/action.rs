use super::{Context, input};
use agent_computer::DesktopActionRequest;
use agent_computer_runtime::{Operation, Reply};
use agent_runtime::ToolCall;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

impl Context {
  pub(super) async fn act(&self, call: ToolCall, cancellation: CancellationToken) -> Result<Value> {
    let input: input::Act = serde_json::from_value(call.arguments)?;
    input.validate()?;
    let action = input.action.action()?;
    self.fence(&cancellation)?;
    let mut state = self.state.lock().await;
    let observed = state
      .observation
      .take()
      .ok_or_else(|| anyhow::anyhow!("observe before input"))?;
    ensure!(
      observed.id == input.snapshot_id && observed.revision == input.revision,
      "stale computer observation"
    );
    let lease = state
      .lease
      .clone()
      .ok_or_else(|| anyhow::anyhow!("computer lease unavailable"))?;
    // A model response can take longer than the native snapshot's 2s lifetime.
    // Refresh against the old content digest, never silently adopt changed content.
    let fresh = self.capture(lease.clone(), cancellation.clone()).await?;
    let fresh = tokio::task::spawn_blocking(move || -> Result<_> {
      ensure!(
        fresh.content_digest()? == observed.digest,
        "desktop content changed"
      );
      Ok((fresh.id, fresh.revision))
    })
    .await??;
    let request = DesktopActionRequest {
      action_id: Uuid::now_v7(),
      lease,
      snapshot_id: fresh.0,
      snapshot_revision: fresh.1,
      action,
    };
    let Reply::Prepared(preparation) = self
      .request(Operation::Prepare { request }, cancellation.clone())
      .await?
    else {
      anyhow::bail!("invalid computer preparation");
    };
    let exact = json!({
      "action_id":preparation.request.action_id,"host_id":self.host.info.host_id,
      "session_id":self.binding.daemon_session_id,"owner_epoch":self.binding.owner_epoch,
      "run_id":self.binding.run_id,"call_id":call.id,"timeout_ms":input.timeout_ms,
      "original_observation":{"snapshot_id":input.snapshot_id,"revision":input.revision},
      "preparation":preparation
    });
    let approval = crate::authorization::wait_for_approval(
      &self.daemon,
      &self.binding.run_id,
      self.binding.owner_epoch,
      "computer_act",
      &exact,
      cancellation.clone(),
    )
    .await?;
    self.fence(&cancellation)?;
    let approval_id = Uuid::parse_str(&approval)?;
    let operation = Operation::Execute {
      preparation_id: preparation.preparation_id,
      action_id: preparation.request.action_id,
      content_digest: preparation.content_digest,
      approval_id,
      timeout_ms: input.timeout_ms,
    };
    let action_id = preparation.request.action_id.to_string();
    let proof = self
      .daemon
      .store
      .begin_approved_operation(agent_storage::ApprovedOperation {
        run_id: &self.binding.run_id,
        owner_epoch: self.binding.owner_epoch,
        action_id: &action_id,
        tool: "computer_act",
        arguments: &exact,
        summary: json!({}),
        approval_id: &approval,
      })?;
    // No await between the committed authorization and its uncertainty guard.
    let mutation = super::super::mutation::CommittedAction::new(
      self.daemon.store.clone(),
      proof,
      self.host.clone(),
      preparation,
      operation,
    );
    self.daemon.events.notify_waiters();
    mutation.execute(&self.daemon, cancellation).await
  }
}
