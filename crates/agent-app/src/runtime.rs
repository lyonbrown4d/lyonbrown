use agent_kernel::Kernel;
use tokio::task::JoinHandle;

use crate::{
  AppController, ApprovalInbox, Projector, RuntimeActorError, RuntimePhase, RuntimeSnapshot,
  channel,
};

mod actor;
mod command;

pub struct RuntimeTask {
  task: JoinHandle<Result<(), RuntimeActorError>>,
}

impl RuntimeTask {
  pub async fn join(self) -> Result<(), RuntimeActorError> {
    self
      .task
      .await
      .map_err(|error| RuntimeActorError::RuntimeTask(error.to_string()))?
  }
}

pub fn spawn_runtime(kernel: Kernel, command_capacity: usize) -> (AppController, RuntimeTask) {
  spawn(kernel, None, command_capacity)
}

pub fn spawn_runtime_with_approvals(
  kernel: Kernel,
  approvals: ApprovalInbox,
  command_capacity: usize,
) -> (AppController, RuntimeTask) {
  spawn(kernel, Some(approvals), command_capacity)
}

fn spawn(
  kernel: Kernel,
  approvals: Option<ApprovalInbox>,
  command_capacity: usize,
) -> (AppController, RuntimeTask) {
  let projector = initial_projector(&kernel);
  let (controller, endpoint) = channel(projector.snapshot().clone(), command_capacity);
  let task = tokio::spawn(actor::run(kernel, endpoint, projector, approvals));
  (controller, RuntimeTask { task })
}

fn initial_projector(kernel: &Kernel) -> Projector {
  let mut projector = Projector::new(RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    session_id: Some(kernel.session_id()),
    ..RuntimeSnapshot::default()
  });
  projector.extend(kernel.event_log().iter().cloned());
  projector
}
