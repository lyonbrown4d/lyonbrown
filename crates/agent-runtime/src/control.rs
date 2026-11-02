//! Product input commands use the Harness queue; they never run a second loop.
use crate::{Result, Runtime, RuntimeError, SessionMutation, SessionPersistence};
use agent_core::events::{AgentEvent, QueueKind, QueueStatus};
use async_trait::async_trait;
use rpi_harness::agent_harness::{AgentLane, CancelQueuedOutcome};
use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, Ordering},
};

pub(crate) struct InputControl {
  pub(crate) admission: tokio::sync::Mutex<bool>,
  request: Mutex<Option<String>>,
  failed: AtomicBool,
}
impl Default for InputControl {
  fn default() -> Self {
    Self {
      admission: tokio::sync::Mutex::new(true),
      request: Mutex::new(None),
      failed: AtomicBool::new(false),
    }
  }
}
pub(crate) struct InputPersistence {
  pub(crate) inner: Arc<dyn SessionPersistence>,
  pub(crate) control: Arc<InputControl>,
}
#[async_trait]
impl SessionPersistence for InputPersistence {
  async fn load(&self, session: &str) -> Result<Option<Vec<u8>>> {
    self.inner.load(session).await
  }
  async fn commit(
    &self,
    session: &str,
    epoch: u64,
    snapshot: Vec<u8>,
    mut mutation: SessionMutation,
  ) -> Result<()> {
    let request = self
      .control
      .request
      .lock()
      .map_err(|_| RuntimeError::Persistence("input control lock unavailable".into()))?
      .clone();
    for event in &mut mutation.events {
      if let AgentEvent::QueueUpdated {
        status: QueueStatus::Pending,
        request_id,
        ..
      } = event
      {
        *request_id = request.clone();
      }
    }
    let result = self.inner.commit(session, epoch, snapshot, mutation).await;
    if result.is_err() {
      self.control.failed.store(true, Ordering::Release);
    }
    result
  }
}
impl Runtime {
  pub async fn enqueue_input(
    &self,
    request_id: &str,
    kind: QueueKind,
    prompt: &str,
  ) -> Result<String> {
    crate::input::validate(prompt, &[])?;
    let open = self.control.admission.lock().await;
    if !*open || self.control.failed.load(Ordering::Acquire) {
      return Err(RuntimeError::Configuration(
        "run input admission is closed".into(),
      ));
    }
    *self
      .control
      .request
      .lock()
      .map_err(|_| RuntimeError::Persistence("input control lock unavailable".into()))? =
      Some(request_id.into());
    let message = rpi_agent::message::AgentMessage::User(rpi_ai::UserMessage::new(
      prompt,
      agent_core::now_ms(),
    ));
    let result = match kind {
      QueueKind::Steer => self.harness.steer(message).await,
      QueueKind::FollowUp => self.harness.follow_up(message).await,
    };
    *self
      .control
      .request
      .lock()
      .map_err(|_| RuntimeError::Persistence("input control lock unavailable".into()))? = None;
    if self.control.failed.load(Ordering::Acquire) {
      let _ = self.harness.abort().await;
      return Err(RuntimeError::Uncertain(
        "input persistence failed; reconcile the existing request identity".into(),
      ));
    }
    result
      .map(|result| result.entry_id)
      .map_err(|error| RuntimeError::Harness(error.to_string()))
  }
  /// True only after the cancellation record committed. Upstream 0.14 may
  /// swallow a storage error; the persistence port fences that ambiguity here.
  /// Upstream: https://github.com/bigfish1913/pi-rust/issues/235
  pub async fn cancel_input(&self, entry_id: &str) -> Result<bool> {
    let _guard = self.control.admission.lock().await;
    if self.control.failed.load(Ordering::Acquire) {
      return Err(RuntimeError::Uncertain(
        "input persistence requires reconciliation".into(),
      ));
    }
    let result = self
      .harness
      .cancel_queued(entry_id)
      .await
      .map_err(|error| RuntimeError::Harness(error.to_string()))?;
    if self.control.failed.load(Ordering::Acquire) {
      let _ = self.harness.abort().await;
      return Err(RuntimeError::Uncertain(
        "input cancellation was not durably confirmed; do not replay".into(),
      ));
    }
    Ok(matches!(result.outcome, CancelQueuedOutcome::Cancelled))
  }
  /// Durable proof for an input no longer present in the in-memory queue.
  pub async fn input_consumed(&self, entry_id: &str) -> Result<bool> {
    let entry = self
      .harness
      .session()
      .get_entry(entry_id)
      .await
      .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
    Ok(
      matches!(entry,Some(rpi_harness::session::types::Entry::Message(value)) if matches!(value.message,rpi_agent::message::AgentMessage::User(_))),
    )
  }
  pub fn input_persistence_failed(&self) -> bool {
    self.control.failed.load(Ordering::Acquire)
  }
  pub async fn close_input_admission(&self) {
    *self.control.admission.lock().await = false;
  }
  pub async fn has_pending_inputs(&self) -> Result<bool> {
    self
      .harness
      .queued_messages()
      .await
      .map(|value| !value.is_empty())
      .map_err(|error| RuntimeError::Harness(error.to_string()))
  }
}
