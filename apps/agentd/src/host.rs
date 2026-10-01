use std::sync::Arc;

use agent_app::{AgentCommand, AppController, RuntimeCommandError, RuntimeSnapshot};
use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct VersionedSnapshot {
  pub revision: u64,
  pub snapshot: RuntimeSnapshot,
}

impl VersionedSnapshot {
  pub fn new(revision: u64, snapshot: RuntimeSnapshot) -> Self {
    Self { revision, snapshot }
  }
}

#[async_trait]
pub trait RuntimeHost: Send + Sync + 'static {
  async fn execute(&self, command: AgentCommand) -> Result<(), RuntimeCommandError>;
  fn snapshot(&self) -> VersionedSnapshot;
  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot>;
}

struct ForwarderGuard {
  cancellation: CancellationToken,
}

impl Drop for ForwarderGuard {
  fn drop(&mut self) {
    self.cancellation.cancel();
  }
}

/// Bridges the current in-process application controller into the daemon boundary.
pub struct AppControllerHost {
  controller: AppController,
  snapshots: watch::Receiver<VersionedSnapshot>,
  _forwarder: Arc<ForwarderGuard>,
}

impl AppControllerHost {
  pub fn new(controller: AppController) -> Self {
    let mut source = controller.subscribe();
    let initial = VersionedSnapshot::new(1, source.borrow().clone());
    let (sender, snapshots) = watch::channel(initial);
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    tokio::spawn(async move {
      let mut revision = 1;
      loop {
        tokio::select! {
          _ = task_cancellation.cancelled() => break,
          changed = source.changed() => {
            if changed.is_err() {
              break;
            }
            revision += 1;
            sender.send_replace(VersionedSnapshot::new(revision, source.borrow_and_update().clone()));
          }
        }
      }
    });
    Self {
      controller,
      snapshots,
      _forwarder: Arc::new(ForwarderGuard { cancellation }),
    }
  }
}

#[async_trait]
impl RuntimeHost for AppControllerHost {
  async fn execute(&self, command: AgentCommand) -> Result<(), RuntimeCommandError> {
    self.controller.execute(command).await
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.clone()
  }
}
