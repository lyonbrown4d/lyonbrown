//! One cancellable consumer executes persisted memory-index intents.
use crate::Daemon;
use memory_core::{MemoryError, MemoryRepository};
use std::time::Duration;

impl Daemon {
  pub(crate) async fn start_memory_worker(&self) -> anyhow::Result<()> {
    let mut installed = self.memory_worker.lock().await;
    anyhow::ensure!(installed.is_none(), "memory worker is already installed");
    let recovered = self.store.recover_jobs().await?;
    tracing::info!(
      recovered,
      "reconciled interrupted memory jobs without replay"
    );
    if !self.config.memory.enabled || self.config.memory.embedding.is_none() {
      return Ok(());
    }
    self
      .memory
      .ensure_rebuild(&agent_memory::MemoryAccess::new(vec![
        memory_core::MemoryScope::User(self.node_id.clone()),
      ]))
      .await?;
    let service = self.memory.clone();
    let cancel = self.stopping.child_token();
    let events = self.events.clone();
    let lease = self._lease.clone();
    // Capture ports and cancellation only. Capturing Daemon would retain its
    // own JoinHandle through RuntimeServices and prevent normal resource drop.
    *installed = Some(tokio::spawn(async move {
      // A replacement daemon cannot take ownership before the cancelled
      // consumer records its final confirmed/uncertain job outcome.
      let _lease = lease;
      loop {
        if cancel.is_cancelled() {
          break;
        }
        match service.process_one(cancel.child_token()).await {
          Ok(true) => {
            events.notify_waiters();
            tokio::task::yield_now().await;
            continue;
          }
          Err(MemoryError::Cancelled) => break,
          Err(_) => {
            tracing::warn!("memory job persistence unavailable; deferring queue inspection")
          }
          Ok(false) => {}
        }
        // Polling bounds queue latency without making RPC delivery responsible
        // for the durability of wakeups. Failed/Uncertain jobs are never retried.
        tokio::select! {
          biased;
          _ = cancel.cancelled() => break,
          _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
      }
    }));
    Ok(())
  }

  pub(crate) async fn stop_memory_worker(&self) {
    if let Some(mut worker) = self.memory_worker.lock().await.take() {
      match tokio::time::timeout(Duration::from_secs(10), &mut worker).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => tracing::error!(
          "memory worker stopped unexpectedly; in-flight jobs require restart reconciliation"
        ),
        Err(_) => {
          // Do not let an unavailable database prevent daemon shutdown. A
          // claimed job remains Running and is fenced as Uncertain on startup.
          worker.abort();
          tracing::error!(
            "memory worker shutdown timed out; in-flight jobs require restart reconciliation"
          );
        }
      }
    }
  }
}
