//! Durable input admission, separate from Rig's model/tool state machine.
use crate::{Result, Runtime, RuntimeError, persistence::QueuedInput};
use agent_core::events::{QueueKind, QueueStatus};
use serde_json::json;
use std::sync::atomic::Ordering;
impl Runtime {
  pub async fn enqueue_input(
    &self,
    request_id: &str,
    kind: QueueKind,
    prompt: &str,
  ) -> Result<String> {
    crate::validate_input(prompt, &[])?;
    if request_id.trim().is_empty() {
      return Err(RuntimeError::Configuration(
        "input request identity is required".into(),
      ));
    }
    self
      .session
      .update("input_enqueued", |state| {
        if let Some(existing) = state
          .queue
          .iter()
          .find(|input| input.request_id == request_id)
        {
          if existing.kind != kind || existing.prompt != prompt {
            return Err(RuntimeError::Configuration(
              "input request identity was reused with different content".into(),
            ));
          }
          return Ok((existing.id.clone(), json!({"entry_id":existing.id}), vec![]));
        }
        if !self.admission.load(Ordering::Acquire) {
          return Err(RuntimeError::Configuration(
            "run input admission is closed".into(),
          ));
        }
        let input = QueuedInput {
          id: agent_core::new_id(),
          request_id: request_id.into(),
          kind,
          prompt: prompt.into(),
          status: QueueStatus::Pending,
        };
        let event = input.event(QueueStatus::Pending);
        let id = input.id.clone();
        state.queue.push(input);
        Ok((id.clone(), json!({"entry_id":id}), vec![event]))
      })
      .await
  }
  pub async fn cancel_input(&self, entry_id: &str) -> Result<bool> {
    self
      .session
      .update("input_cancelled", |state| {
        let Some(input) = state
          .queue
          .iter_mut()
          .find(|input| input.id == entry_id && input.status == QueueStatus::Pending)
        else {
          return Ok((false, json!({"entry_id":entry_id}), vec![]));
        };
        input.status = QueueStatus::Cancelled;
        Ok((
          true,
          json!({"entry_id":entry_id}),
          vec![input.event(QueueStatus::Cancelled)],
        ))
      })
      .await
  }
  pub async fn input_consumed(&self, entry_id: &str) -> Result<bool> {
    Ok(
      self
        .session
        .state
        .lock()
        .await
        .queue
        .iter()
        .any(|input| input.id == entry_id && input.status == QueueStatus::Consumed),
    )
  }
  pub fn input_persistence_failed(&self) -> bool {
    self.session.failed.load(Ordering::Acquire)
  }
  pub async fn close_input_admission(&self) {
    // The same session lock orders closure against a concurrent admission commit.
    let _state = self.session.state.lock().await;
    self.admission.store(false, Ordering::Release);
  }
  pub async fn has_pending_inputs(&self) -> Result<bool> {
    Ok(
      self
        .session
        .state
        .lock()
        .await
        .queue
        .iter()
        .any(|input| input.status == QueueStatus::Pending),
    )
  }
}
