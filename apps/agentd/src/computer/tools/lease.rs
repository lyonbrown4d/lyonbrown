use super::Context;
use agent_computer::LeaseToken;
use agent_computer_runtime::{Operation, Reply};
use anyhow::{Result, ensure};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

impl Context {
  pub(super) async fn renew(
    &self,
    lease: &LeaseToken,
    cancellation: CancellationToken,
  ) -> Result<()> {
    let reply = self
      .request(
        Operation::RenewLease {
          lease: lease.clone(),
          ttl_ms: self.lease_ms,
        },
        cancellation,
      )
      .await?;
    ensure!(
      matches!(reply, Reply::Lease(ref renewed) if renewed == lease),
      "computer lease changed"
    );
    Ok(())
  }
}

/// Keep the existing live controller through model and human approval latency.
/// Renewal never acquires a new token or revives a revoked/expired controller.
pub(super) fn supervise(
  context: &Arc<Context>,
  cancellation: CancellationToken,
) -> tokio::task::JoinHandle<()> {
  let period = Duration::from_millis(context.lease_ms / 3);
  let context = Arc::downgrade(context);
  tokio::spawn(async move {
    let mut ticks = tokio::time::interval(period);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticks.tick().await;
    loop {
      tokio::select! { biased;
        _ = cancellation.cancelled() => break,
        _ = ticks.tick() => {}
      }
      let Some(context) = context.upgrade() else {
        break;
      };
      if context.fence(&cancellation).is_err() {
        break;
      }
      let lease = context.lease_watch.borrow().clone();
      let Some(lease) = lease else { continue };
      let result = context
        .daemon
        .computer
        .request(
          &context.host,
          context.binding.clone(),
          Operation::RenewLease {
            lease,
            ttl_ms: context.lease_ms,
          },
          cancellation.clone(),
        )
        .await;
      match result {
        Ok(Ok(Reply::Lease(_))) => {}
        _ => {
          context
            .daemon
            .computer
            .disconnect(&context.binding.daemon_session_id, context.host.generation)
            .await;
          break;
        }
      }
    }
  })
}
