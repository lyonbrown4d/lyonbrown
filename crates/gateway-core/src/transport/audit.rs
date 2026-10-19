use crate::{AuditRecord, AuditSink, AuditStatus, GatewayError};
use std::sync::Arc;
use tokio::{
  sync::{OwnedSemaphorePermit, oneshot},
  time::{Instant, timeout_at},
};

/// Completion is tied to the response body's exhaustion or drop, including an unpolled body.
pub(super) struct BodyCompletion(Option<oneshot::Sender<()>>);
impl Drop for BodyCompletion {
  fn drop(&mut self) {
    if let Some(send) = self.0.take() {
      let _ = send.send(());
    }
  }
}
pub(super) struct AuditGuard {
  sink: Arc<dyn AuditSink>,
  record: AuditRecord,
  started: Instant,
  terminal: bool,
  deadline: Instant,
  signals: Arc<crate::signals::Signals>,
  permit: Option<OwnedSemaphorePermit>,
  body_done: Option<oneshot::Receiver<()>>,
}
impl AuditGuard {
  pub(super) fn new(
    sink: Arc<dyn AuditSink>,
    record: AuditRecord,
    started: Instant,
    deadline: Instant,
    signals: Arc<crate::signals::Signals>,
    permit: OwnedSemaphorePermit,
  ) -> Self {
    Self {
      sink,
      record,
      started,
      terminal: false,
      deadline,
      signals,
      permit: Some(permit),
      body_done: None,
    }
  }
  pub(super) fn request_id(&self) -> &str {
    &self.record.request_id
  }
  pub(super) fn body_completion(&mut self) -> BodyCompletion {
    let (send, receive) = oneshot::channel();
    self.body_done = Some(receive);
    BodyCompletion(Some(send))
  }
  /// A timed-out caller must not cancel a SQLite worker that may still commit Pending.
  pub(super) async fn prepare(mut self, first_deadline: Instant) -> Result<Self, GatewayError> {
    let (send, receive) = oneshot::channel();
    tokio::spawn(async move {
      if self.sink.record(self.record.clone()).await.is_err() {
        self.terminal = true;
        drop(self);
        return;
      }
      if let Err(guard) = send.send(self) {
        // The caller cannot dispatch upstream after this receiver has disappeared.
        let _ = guard.finish(AuditStatus::Failed).await;
      }
    });
    timeout_at(first_deadline, receive)
      .await
      .map_err(|_| GatewayError::unavailable())?
      .map_err(|_| GatewayError::unavailable())
  }
  /// An owned writer retains both terminal responsibility and admission after an HTTP timeout.
  /// The acknowledgment precedes the body lease wait, so awaiting EOF cannot deadlock the writer.
  pub(super) async fn finish(mut self, status: AuditStatus) -> Result<(), GatewayError> {
    self.record.status = status;
    self.record.elapsed_ms = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let deadline = self.deadline;
    let (send, receive) = oneshot::channel();
    tokio::spawn(async move {
      let result = self.sink.record(self.record.clone()).await;
      if result.is_ok() {
        self.terminal = true;
        self.signals.terminal(status, self.record.elapsed_ms);
        tracing::info!(request_id=%self.record.request_id, model=%self.record.model, status=?status, elapsed_ms=self.record.elapsed_ms, policy_revision=%self.record.policy_revision, "gateway request terminated");
      }
      let _ = send.send(result.map_err(|_| GatewayError::unavailable()));
      if self.terminal
        && let Some(done) = self.body_done.take()
      {
        let _ = timeout_at(self.deadline, done).await;
      }
    });
    timeout_at(deadline, receive)
      .await
      .map_err(|_| GatewayError::unavailable())?
      .map_err(|_| GatewayError::unavailable())?
  }
}
impl Drop for AuditGuard {
  fn drop(&mut self) {
    if self.terminal {
      return;
    }
    let mut record = self.record.clone();
    record.status = AuditStatus::Uncertain;
    record.elapsed_ms = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    self
      .signals
      .terminal(AuditStatus::Uncertain, record.elapsed_ms);
    tracing::warn!(request_id=%record.request_id, model=%record.model, "gateway request dropped with uncertain upstream outcome");
    let sink = self.sink.clone();
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
      return;
    };
    let permit = self.permit.take();
    let body_done = self.body_done.take();
    let deadline = self.deadline;
    runtime.spawn(async move {
      let _permit = permit;
      if sink.record(record).await.is_err() {
        tracing::error!("gateway uncertain audit update failed; pending intent requires recovery");
      }
      if let Some(done) = body_done {
        let _ = timeout_at(deadline, done).await;
      }
    });
  }
}
