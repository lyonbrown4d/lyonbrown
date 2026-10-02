use agent_kernel::Kernel;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
  AppController, ApprovalInbox, Projector, RuntimeActorError, RuntimePhase, RuntimeSnapshot,
  channel,
};

mod actor;
mod command;

pub struct RuntimeTask {
  task: Option<JoinHandle<Result<(), RuntimeActorError>>>,
  shutdown: RuntimeShutdownHandle,
}

/// Cloneable control handle for graceful or bounded runtime termination.
#[derive(Clone, Debug)]
pub struct RuntimeShutdownHandle {
  shutdown: CancellationToken,
  abort: AbortHandle,
}

impl RuntimeShutdownHandle {
  /// Ask the actor to stop accepting work and close its storage after cleanup.
  pub fn request_shutdown(&self) {
    self.shutdown.cancel();
  }

  /// Abort the actor itself when graceful shutdown cannot finish in time.
  pub fn abort(&self) {
    self.shutdown.cancel();
    self.abort.abort();
  }

  pub fn is_shutdown_requested(&self) -> bool {
    self.shutdown.is_cancelled()
  }
}

impl RuntimeTask {
  pub fn shutdown_handle(&self) -> RuntimeShutdownHandle {
    self.shutdown.clone()
  }

  pub fn abort(&self) {
    self.shutdown.abort();
  }

  pub async fn join(mut self) -> Result<(), RuntimeActorError> {
    self
      .task
      .take()
      .expect("runtime task can only be joined once")
      .await
      .map_err(|error| RuntimeActorError::RuntimeTask(error.to_string()))?
  }
}

impl Drop for RuntimeTask {
  fn drop(&mut self) {
    if let Some(task) = self.task.take() {
      self.shutdown.abort();
      task.abort();
    }
  }
}

pub fn spawn_runtime(kernel: Kernel, command_capacity: usize) -> (AppController, RuntimeTask) {
  spawn(kernel, None, RuntimePhase::Ready, command_capacity)
}

pub fn spawn_runtime_waiting_for_model(
  kernel: Kernel,
  command_capacity: usize,
) -> (AppController, RuntimeTask) {
  spawn(
    kernel,
    None,
    RuntimePhase::ModelSetupRequired,
    command_capacity,
  )
}

pub fn spawn_runtime_with_approvals(
  kernel: Kernel,
  approvals: ApprovalInbox,
  command_capacity: usize,
) -> (AppController, RuntimeTask) {
  spawn(
    kernel,
    Some(approvals),
    RuntimePhase::Ready,
    command_capacity,
  )
}

fn spawn(
  kernel: Kernel,
  approvals: Option<ApprovalInbox>,
  initial_phase: RuntimePhase,
  command_capacity: usize,
) -> (AppController, RuntimeTask) {
  let projector = initial_projector(&kernel, initial_phase);
  let (controller, endpoint) = channel(projector.snapshot().clone(), command_capacity);
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(actor::run(
    kernel,
    endpoint,
    projector,
    approvals,
    shutdown.clone(),
  ));
  let shutdown = RuntimeShutdownHandle {
    shutdown,
    abort: task.abort_handle(),
  };
  (
    controller,
    RuntimeTask {
      task: Some(task),
      shutdown,
    },
  )
}

fn initial_projector(kernel: &Kernel, initial_phase: RuntimePhase) -> Projector {
  let mut projector = Projector::new(RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    session_id: Some(kernel.session_id()),
    ..RuntimeSnapshot::default()
  });
  projector.extend(kernel.event_log().iter().cloned());
  projector.set_phase(initial_phase);
  projector
}
