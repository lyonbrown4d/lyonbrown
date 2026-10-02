use std::sync::Arc;

use agent_app::{AgentCommand, RuntimeCommandError};
use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::{CAPABILITY_MODEL_SETUP_DOWNLOAD_V1, RuntimeHost, VersionedSnapshot};
use crate::model_setup::ModelSetupCoordinator;

const MODEL_SETUP_CAPABILITIES: &[&str] = &[CAPABILITY_MODEL_SETUP_DOWNLOAD_V1];

struct ForwarderGuard(CancellationToken);

impl Drop for ForwarderGuard {
  fn drop(&mut self) {
    self.0.cancel();
  }
}

pub struct ManagedRuntimeHost {
  inner: Arc<dyn RuntimeHost>,
  coordinator: ModelSetupCoordinator,
  snapshots: watch::Sender<VersionedSnapshot>,
  advertises_download: bool,
  _forwarder: ForwarderGuard,
}

impl ManagedRuntimeHost {
  pub fn new(inner: Arc<dyn RuntimeHost>, coordinator: ModelSetupCoordinator) -> Self {
    let advertises_download = coordinator.advertises_download();
    let mut runtime = inner.subscribe();
    let mut setup = coordinator.subscribe();
    let mut initial = runtime.borrow().clone();
    initial.snapshot.model_setup = setup.borrow().clone();
    let (snapshots, _) = watch::channel(initial);
    let sender = snapshots.clone();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    tokio::spawn(async move {
      let mut revision = sender.borrow().revision;
      loop {
        tokio::select! {
          _ = task_cancellation.cancelled() => break,
          changed = runtime.changed() => {
            if changed.is_err() {
              break;
            }
            let mut snapshot = runtime.borrow_and_update().snapshot.clone();
            snapshot.model_setup = setup.borrow().clone();
            revision = revision.saturating_add(1);
            sender.send_replace(VersionedSnapshot::new(revision, snapshot));
          }
          changed = setup.changed() => {
            if changed.is_err() {
              break;
            }
            let mut snapshot = runtime.borrow().snapshot.clone();
            snapshot.model_setup = setup.borrow_and_update().clone();
            revision = revision.saturating_add(1);
            sender.send_replace(VersionedSnapshot::new(revision, snapshot));
          }
        }
      }
    });
    Self {
      inner,
      coordinator,
      snapshots,
      advertises_download,
      _forwarder: ForwarderGuard(cancellation),
    }
  }
}

#[async_trait]
impl RuntimeHost for ManagedRuntimeHost {
  async fn execute(&self, command: AgentCommand) -> Result<(), RuntimeCommandError> {
    match command {
      AgentCommand::StartModelDownload { model_id } => self.coordinator.start(model_id).await,
      AgentCommand::CancelModelDownload { operation_id } => {
        self.coordinator.cancel(operation_id).await
      }
      command => self.inner.execute(command).await,
    }
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }

  fn capabilities(&self) -> &'static [&'static str] {
    if self.advertises_download {
      MODEL_SETUP_CAPABILITIES
    } else {
      &[]
    }
  }
}
