use std::sync::Arc;

use agent_app::{AgentClient, AgentClientError, AgentCommand, AvailabilityKind};
use tokio::task::JoinSet;

/// Keeps command RPCs off the terminal event loop while retaining cancellation ownership.
pub struct CommandDispatcher {
  tasks: JoinSet<Result<(), agent_app::AgentClientError>>,
}

impl CommandDispatcher {
  pub fn new() -> Self {
    Self {
      tasks: JoinSet::new(),
    }
  }

  pub fn dispatch(&mut self, client: Arc<dyn AgentClient>, command: AgentCommand) {
    self
      .tasks
      .spawn(async move { client.execute(command).await });
  }

  pub fn drain_failures(&mut self) -> Vec<AgentClientError> {
    let mut failures = Vec::new();
    while let Some(result) = self.tasks.try_join_next() {
      match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => failures.push(error),
        Err(error) if error.is_cancelled() => {}
        Err(error) => failures.push(AgentClientError::unavailable(
          AvailabilityKind::ShuttingDown,
          format!("command task failed: {error}"),
        )),
      }
    }
    failures
  }
}

impl Default for CommandDispatcher {
  fn default() -> Self {
    Self::new()
  }
}
