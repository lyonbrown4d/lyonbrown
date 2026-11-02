//! Final flush paths: quit hooks and view replacement release the store lock.
use crate::local_state::DesktopState;
use crate::persistence::worker::Command;
use crate::shell::Desktop;
use tokio::sync::mpsc::Sender;

pub(crate) type FlushControl = std::sync::Arc<std::sync::Mutex<FlushState>>;

#[derive(Default)]
pub(crate) struct FlushState {
  pub(crate) sender: Option<Sender<Command>>,
  pub(crate) scope: Option<String>,
  pub(crate) generation: u64,
  pub(crate) sequence: u64,
  pub(crate) state: Option<DesktopState>,
  pub(crate) completion: Option<tokio::sync::oneshot::Receiver<()>>,
}

pub(crate) async fn quit_flush(control: FlushControl) {
  let pending = {
    let Ok(mut state) = control.lock() else {
      return;
    };
    if let Some(completion) = state.completion.take() {
      Some((None, completion))
    } else if let (Some(sender), Some(scope)) = (state.sender.take(), state.scope.take()) {
      let (command, completion) =
        Command::stop(scope, state.generation, state.sequence, state.state.take());
      Some((Some((sender, command)), completion))
    } else {
      None
    }
  };
  if let Some((send, completion)) = pending {
    if let Some((sender, command)) = send {
      let _ = sender.send(command).await;
    }
    let _ = completion.await;
  }
}

impl Desktop {
  /// Flush pending local state and release its exclusive store lock.
  ///
  /// The desktop normally calls this from the application quit hook. It is
  /// also useful to coordinate a view replacement before reopening the same
  /// client state directory.
  #[doc(hidden)]
  pub fn flush_local_state(
    &mut self,
  ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>> {
    let sender = self.persistence_sender.take();
    let scope = self.persistence_scope.take();
    let generation = self.persistence_generation;
    let sequence = self.persistence_sequence;
    let state = self.latest_state.clone();
    if let Ok(mut flush) = self.flush_control.lock() {
      flush.sender = None;
      flush.scope = None;
      flush.state = None;
      flush.completion = None;
    }
    let should_wait = if let (Some(sender), Some(scope)) = (sender, scope) {
      let (command, completion) = Command::stop(scope, generation, sequence, state);
      self.runtime.spawn(async move {
        let _ = sender.send(command).await;
      });
      Some(completion)
    } else {
      None
    };
    Box::pin(async move {
      if let Some(completion) = should_wait {
        let _ = completion.await;
      }
    })
  }
}
