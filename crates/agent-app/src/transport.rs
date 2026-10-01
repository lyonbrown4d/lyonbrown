use tokio::sync::{mpsc, oneshot, watch};

use crate::{AgentCommand, RuntimeCommandError, RuntimeSnapshot};

pub(crate) struct CommandRequest {
  pub(crate) command: AgentCommand,
  response: Option<oneshot::Sender<Result<(), RuntimeCommandError>>>,
}

impl CommandRequest {
  pub(crate) fn respond(self, result: Result<(), RuntimeCommandError>) {
    if let Some(response) = self.response {
      let _ = response.send(result);
    }
  }
}

/// Publishing failed because every snapshot subscriber was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotSendError;

impl std::fmt::Display for SnapshotSendError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str("all runtime snapshot subscribers were dropped")
  }
}

impl std::error::Error for SnapshotSendError {}

/// Cloneable UI-side handle for commands and projected runtime state.
#[derive(Debug, Clone)]
pub struct AppController {
  commands: mpsc::Sender<CommandRequest>,
  snapshots: watch::Receiver<RuntimeSnapshot>,
}

impl AppController {
  pub async fn send(&self, command: AgentCommand) -> Result<(), RuntimeCommandError> {
    self
      .commands
      .send(CommandRequest {
        command,
        response: None,
      })
      .await
      .map_err(|_| RuntimeCommandError::RuntimeStopped)
  }

  pub async fn execute(&self, command: AgentCommand) -> Result<(), RuntimeCommandError> {
    let (response_tx, response_rx) = oneshot::channel();
    self
      .commands
      .send(CommandRequest {
        command,
        response: Some(response_tx),
      })
      .await
      .map_err(|_| RuntimeCommandError::RuntimeStopped)?;
    response_rx
      .await
      .map_err(|_| RuntimeCommandError::RuntimeStopped)?
  }

  pub fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.snapshots.clone()
  }

  pub fn snapshot(&self) -> RuntimeSnapshot {
    self.snapshots.borrow().clone()
  }
}

/// Runtime-actor side of the transport. It deliberately does not own a Kernel.
#[derive(Debug)]
pub struct RuntimeEndpoint {
  commands: mpsc::Receiver<CommandRequest>,
  snapshots: watch::Sender<RuntimeSnapshot>,
}

impl RuntimeEndpoint {
  pub async fn recv(&mut self) -> Option<AgentCommand> {
    let request = self.commands.recv().await?;
    let command = request.command.clone();
    request.respond(Ok(()));
    Some(command)
  }

  pub(crate) async fn recv_request(&mut self) -> Option<CommandRequest> {
    self.commands.recv().await
  }

  pub fn publish(&self, snapshot: RuntimeSnapshot) -> Result<(), SnapshotSendError> {
    self.snapshots.send(snapshot).map_err(|_| SnapshotSendError)
  }
}

/// Create a bounded command channel and a watch channel for the latest snapshot.
pub fn channel(
  initial: RuntimeSnapshot,
  command_capacity: usize,
) -> (AppController, RuntimeEndpoint) {
  let (command_tx, command_rx) = mpsc::channel(command_capacity.max(1));
  let (snapshot_tx, snapshot_rx) = watch::channel(initial);
  (
    AppController {
      commands: command_tx,
      snapshots: snapshot_rx,
    },
    RuntimeEndpoint {
      commands: command_rx,
      snapshots: snapshot_tx,
    },
  )
}
