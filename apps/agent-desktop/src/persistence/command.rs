//! Commands and results exchanged with the persistence actor.
use crate::local_state::DesktopState;

pub(crate) type LatestSnapshot = std::sync::Arc<std::sync::Mutex<Option<(u64, DesktopState)>>>;

pub(crate) struct ActorConfig {
  pub(crate) root: std::path::PathBuf,
  pub(crate) endpoint: String,
  pub(crate) node_id: String,
  pub(crate) scope: String,
  pub(crate) generation: u64,
}

#[derive(Debug)]
pub(crate) enum Command {
  Save {
    scope: String,
    generation: u64,
    sequence: u64,
  },
  Stop {
    scope: String,
    generation: u64,
    sequence: u64,
    state: Option<Box<DesktopState>>,
    ack: Option<tokio::sync::oneshot::Sender<()>>,
  },
}

impl Command {
  /// Build the final flush plus the completion signal that resolves once the
  /// store lock is released.
  ///
  /// Every teardown path shares this constructor: replacing the authenticated
  /// scope, the quit hook, the explicit view flush and `Desktop::drop`.
  pub(crate) fn stop(
    scope: String,
    generation: u64,
    sequence: u64,
    state: Option<DesktopState>,
  ) -> (Self, tokio::sync::oneshot::Receiver<()>) {
    let (ack, completion) = tokio::sync::oneshot::channel();
    (
      Self::Stop {
        scope,
        generation,
        sequence,
        state: state.map(Box::new),
        ack: Some(ack),
      },
      completion,
    )
  }
}

#[derive(Debug)]
pub(crate) enum ResultMessage {
  Loaded {
    scope: String,
    generation: u64,
    state: Option<Box<DesktopState>>,
  },
  Failed {
    scope: String,
    generation: u64,
    message: String,
  },
  Saved {
    scope: String,
    generation: u64,
    sequence: u64,
  },
  SaveFailed {
    scope: String,
    generation: u64,
    sequence: u64,
    message: String,
  },
}
