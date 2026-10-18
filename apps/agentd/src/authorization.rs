//! Durable exact-action approval shared by daemon-owned tool adapters.
use crate::Daemon;
use agent_core::now_ms;
use anyhow::{Result, ensure};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub(crate) async fn approve(
  daemon: &Daemon,
  run: &str,
  epoch: u64,
  tool: &str,
  arguments: &Value,
  cancellation: CancellationToken,
) -> Result<()> {
  let approval = wait_for_approval(daemon, run, epoch, tool, arguments, cancellation).await?;
  ensure!(
    daemon
      .store
      .consume_approval_owned(&approval, run, epoch, tool, arguments)?,
    "operation was not approved"
  );
  Ok(())
}

/// Wait for the human decision without consuming it. Native tools consume the
/// exact approval in the same storage transaction as their execution intent.
pub(crate) async fn wait_for_approval(
  daemon: &Daemon,
  run: &str,
  epoch: u64,
  tool: &str,
  arguments: &Value,
  cancellation: CancellationToken,
) -> Result<String> {
  ensure!(
    !cancellation.is_cancelled(),
    "tool cancelled before approval"
  );
  let approval = daemon
    .store
    .request_approval_owned(run, epoch, tool, arguments)?;
  daemon.events.notify_waiters();
  loop {
    let changed = daemon.approvals.notified();
    tokio::pin!(changed);
    changed.as_mut().enable();
    let current = daemon
      .store
      .approval(&approval.id)?
      .ok_or_else(|| anyhow::anyhow!("approval missing"))?;
    ensure!(
      !cancellation.is_cancelled(),
      "tool cancelled before authorization"
    );
    if current.status != "pending" {
      ensure!(current.status == "approved", "operation was not approved");
      return Ok(approval.id);
    }
    let remaining = current.expires_at_ms - now_ms();
    ensure!(remaining > 0, "approval expired");
    tokio::select! {
      _ = changed => {},
      _ = cancellation.cancelled() => anyhow::bail!("tool cancelled before approval"),
      _ = tokio::time::sleep(std::time::Duration::from_millis(remaining as u64)) => anyhow::bail!("approval expired"),
    }
  }
}
