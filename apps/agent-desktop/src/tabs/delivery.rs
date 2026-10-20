use super::ScopedUpdate;
use crate::connection::Update;

#[derive(Clone)]
pub(crate) struct TabSender {
  pub(crate) id: u64,
  pub(crate) sender: async_channel::Sender<ScopedUpdate>,
}

impl TabSender {
  pub async fn send(&self, update: Update) -> bool {
    self
      .sender
      .send(ScopedUpdate::Tab {
        id: self.id,
        update,
      })
      .await
      .is_ok()
  }

  pub async fn finish_request(&self) {
    let _ = self.sender.send(ScopedUpdate::RequestFinished).await;
  }

  pub fn attach_watch(&self, task: tokio::task::JoinHandle<()>) -> impl Future<Output = ()> + '_ {
    // Wrap before constructing the future, so even an unpolled send owns cleanup.
    let task = PendingWatch(Some(task));
    async move {
      let _ = self
        .sender
        .send(ScopedUpdate::Watch { id: self.id, task })
        .await;
    }
  }
}

/// A watch is local presentation work. Until the UI accepts ownership, dropping
/// its queued acknowledgement or cancelled send must abort it, never detach it.
pub(crate) struct PendingWatch(Option<tokio::task::JoinHandle<()>>);

impl PendingWatch {
  pub(super) fn into_inner(mut self) -> Option<tokio::task::JoinHandle<()>> {
    self.0.take()
  }
}

impl Drop for PendingWatch {
  fn drop(&mut self) {
    if let Some(task) = self.0.take() {
      task.abort();
    }
  }
}
