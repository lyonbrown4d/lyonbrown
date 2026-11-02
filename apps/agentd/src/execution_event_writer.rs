//! Ordered, bounded bridge from the synchronous runtime listener to async persistence.
use agent_core::events::{AGENT_EVENT_KIND, AgentEvent, ModelIdentity};
use agent_runtime::{RuntimeError, RuntimeEvent};
use std::sync::Arc;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

enum Message {
  Event(RuntimeEvent),
  Drain(oneshot::Sender<bool>),
}

pub(crate) struct EventWriter {
  send: mpsc::Sender<Message>,
}
impl EventWriter {
  pub(crate) fn start(
    store: agent_storage::Store,
    run_id: String,
    epoch: u64,
    model: ModelIdentity,
    notify: Arc<Notify>,
    cancel: CancellationToken,
  ) -> (Self, Arc<dyn Fn(RuntimeEvent) + Send + Sync>) {
    let (send, mut receive) = mpsc::channel(128);
    let overflow = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let failed = overflow.clone();
    let stop = cancel.clone();
    tokio::spawn(async move {
      let mut previous = None;
      while let Some(message) = receive.recv().await {
        match message {
          Message::Drain(reply) => {
            let _ = reply.send(!failed.load(std::sync::atomic::Ordering::Acquire));
          }
          Message::Event(event) => {
            for event in crate::execution_events::project(event, &model, &run_id) {
              if let AgentEvent::PhaseChanged { phase, .. } = &event {
                if previous == Some(*phase) {
                  continue;
                }
                previous = Some(*phase);
              }
              let result = match serde_json::to_value(event) {
                Ok(event) => store
                  .append_event_owned(&run_id, epoch, AGENT_EVENT_KIND, event)
                  .await
                  .map(|_| ()),
                Err(error) => Err(error.into()),
              };
              if result.is_err() {
                failed.store(true, std::sync::atomic::Ordering::Release);
                stop.cancel();
              }
            }
            notify.notify_waiters();
          }
        }
      }
    });
    let events = send.clone();
    let listener = Arc::new(move |event| {
      if events.try_send(Message::Event(event)).is_err() {
        overflow.store(true, std::sync::atomic::Ordering::Release);
        cancel.cancel();
      }
    });
    (Self { send }, listener)
  }

  /// A FIFO barrier ensures all accepted events commit before the caller writes
  /// the terminal Run state. Queue overflow or storage failure fences completion.
  pub(crate) async fn drain(&self) -> Result<(), RuntimeError> {
    let (reply, result) = oneshot::channel();
    self
      .send
      .send(Message::Drain(reply))
      .await
      .map_err(|_| Self::failure())?;
    if result.await.map_err(|_| Self::failure())? {
      Ok(())
    } else {
      Err(Self::failure())
    }
  }

  fn failure() -> RuntimeError {
    RuntimeError::Uncertain(
      "runtime event persistence failed; execution requires reconciliation".into(),
    )
  }
}
