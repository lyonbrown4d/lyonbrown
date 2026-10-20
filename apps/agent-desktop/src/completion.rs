//! Lifecycle ownership for executor-neutral completion channels.
use async_channel::{Receiver, RecvError, Sender};

pub(crate) fn channel<T>(capacity: usize) -> (Sender<T>, CompletionReceiver<T>) {
  let (sender, receiver) = async_channel::bounded(capacity);
  (sender, CompletionReceiver(receiver))
}

/// Owned by the weak GPUI receiver task. Closing that task must also discard
/// queued hosts and watch handles, even while an in-flight producer owns a sender.
pub(crate) struct CompletionReceiver<T>(Receiver<T>);

impl<T> CompletionReceiver<T> {
  pub(crate) async fn recv(&self) -> Result<T, RecvError> {
    self.0.recv().await
  }
}

impl<T> Drop for CompletionReceiver<T> {
  fn drop(&mut self) {
    self.0.close();
    while self.0.try_recv().is_ok() {}
  }
}
