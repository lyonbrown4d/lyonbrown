use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};

use agent_storage::{OperationStatus, Store};
use serde_json::json;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Reconciles an admitted command intent if its execution future is dropped.
pub(super) struct ReceiptGuard {
  store: Store,
  events: Arc<Notify>,
  run: String,
  epoch: u64,
  action: String,
  began: Arc<AtomicBool>,
  cancellation: CancellationToken,
  finished: bool,
}

impl ReceiptGuard {
  pub fn new(
    store: Store,
    events: Arc<Notify>,
    run: String,
    epoch: u64,
    action: String,
    began: Arc<AtomicBool>,
    cancellation: CancellationToken,
  ) -> Self {
    Self {
      store,
      events,
      run,
      epoch,
      action,
      began,
      cancellation,
      finished: false,
    }
  }

  pub fn finish(&mut self) {
    self.finished = true;
  }
}

impl Drop for ReceiptGuard {
  fn drop(&mut self) {
    if self.finished || !self.began.load(Ordering::Acquire) {
      return;
    }
    let store = self.store.clone();
    let events = self.events.clone();
    let run = self.run.clone();
    let action = self.action.clone();
    let epoch = self.epoch;
    let summary = if self.cancellation.is_cancelled() {
      json!({"reason":"cancelled"})
    } else {
      json!({"status":"uncertain"})
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
      tracing::warn!(action_id=%action, "command intent remains unresolved; reconcile on startup");
      return;
    };
    runtime.spawn(async move {
      if store.finish_operation(&run, epoch, &action, OperationStatus::Uncertain, summary).await.is_ok() {
        events.notify_waiters();
      } else {
        tracing::warn!(action_id=%action, "command intent may remain unresolved; reconcile on startup");
      }
    });
  }
}
