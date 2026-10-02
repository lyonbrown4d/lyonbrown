use std::future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_app::{AgentClient, AgentClientError, AgentCommand, AvailabilityKind, RuntimeSnapshot};
use agent_cli::tui::runner::{CommandDispatcher, execute_control_with_timeout};
use async_trait::async_trait;
use tokio::sync::watch;

#[derive(Clone, Copy)]
enum ExecuteOutcome {
  Pending,
  Unavailable,
  PendingStartOnly,
}

struct FakeClient {
  commands: Mutex<Vec<AgentCommand>>,
  snapshots: watch::Receiver<RuntimeSnapshot>,
  outcome: ExecuteOutcome,
}

impl FakeClient {
  fn new(outcome: ExecuteOutcome) -> Self {
    let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
    Self {
      commands: Mutex::new(Vec::new()),
      snapshots,
      outcome,
    }
  }

  fn commands(&self) -> Vec<AgentCommand> {
    self.commands.lock().unwrap().clone()
  }
}

#[async_trait]
impl AgentClient for FakeClient {
  async fn execute(&self, command: AgentCommand) -> Result<(), AgentClientError> {
    self.commands.lock().unwrap().push(command);
    match self.outcome {
      ExecuteOutcome::Pending => future::pending().await,
      ExecuteOutcome::Unavailable => Err(AgentClientError::unavailable(
        AvailabilityKind::Disconnected,
        "daemon offline",
      )),
      ExecuteOutcome::PendingStartOnly => match self.commands().last() {
        Some(AgentCommand::StartTurn { .. }) => future::pending().await,
        _ => Ok(()),
      },
    }
  }

  fn snapshot(&self) -> RuntimeSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.snapshots.clone()
  }
}

#[tokio::test]
async fn cancel_turn_times_out_with_an_explicit_action_error() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::Pending));

  let error = execute_control_with_timeout(
    client.as_ref(),
    AgentCommand::CancelTurn,
    Duration::from_millis(10),
  )
  .await
  .unwrap_err();

  assert_eq!(
    error.to_string(),
    "cancel turn command timed out after 10ms"
  );
  assert_eq!(client.commands(), [AgentCommand::CancelTurn]);
}

#[tokio::test]
async fn force_stop_propagates_the_remote_failure() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::Unavailable));

  let error = execute_control_with_timeout(
    client.as_ref(),
    AgentCommand::ForceStop,
    Duration::from_millis(10),
  )
  .await
  .unwrap_err();

  assert!(error.to_string().contains("force-stop turn command failed"));
  assert!(error.to_string().contains("daemon offline"));
  assert_eq!(client.commands(), [AgentCommand::ForceStop]);
}

#[tokio::test]
async fn pending_submission_does_not_block_a_control_command() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::PendingStartOnly));
  let mut dispatcher = CommandDispatcher::new();
  let port: Arc<dyn AgentClient> = client.clone();

  dispatcher.dispatch(
    port,
    AgentCommand::StartTurn {
      text: "keep working".into(),
    },
  );
  tokio::task::yield_now().await;

  execute_control_with_timeout(
    client.as_ref(),
    AgentCommand::CancelTurn,
    Duration::from_millis(25),
  )
  .await
  .unwrap();

  assert_eq!(
    client.commands(),
    [
      AgentCommand::StartTurn {
        text: "keep working".into(),
      },
      AgentCommand::CancelTurn,
    ]
  );
}

#[tokio::test]
async fn background_command_failure_is_reported_without_ending_the_ui_loop() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::Unavailable));
  let mut dispatcher = CommandDispatcher::new();
  let port: Arc<dyn AgentClient> = client;

  dispatcher.dispatch(
    port,
    AgentCommand::StartTurn {
      text: "try once".into(),
    },
  );
  tokio::task::yield_now().await;

  let failures = dispatcher.drain_failures();
  assert_eq!(failures.len(), 1);
  assert!(failures[0].to_string().contains("daemon offline"));
  assert!(dispatcher.drain_failures().is_empty());
}
