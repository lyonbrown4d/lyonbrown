use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_app::{
  AgentClient, AgentClientError, AgentCommand, AvailabilityKind, ManagedModelView,
  ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView, RuntimePhase,
  RuntimeSnapshot,
};
use agent_cli::tui::runner::{
  CommandDispatcher, ModelSetupCommandGate, ModelSetupCompletion, ModelSetupExit, ModelSetupInput,
  execute_control_for_server_with_timeout,
};
use agent_cli::tui::{TuiState, apply_remote_state};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq)]
struct RecordedCommand {
  expected_server_instance_id: Option<String>,
  command: AgentCommand,
}

struct RecordingClient {
  commands: Mutex<Vec<RecordedCommand>>,
  snapshots: watch::Receiver<RuntimeSnapshot>,
}

impl RecordingClient {
  fn new() -> Self {
    let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
    Self {
      commands: Mutex::new(Vec::new()),
      snapshots,
    }
  }

  fn commands(&self) -> Vec<RecordedCommand> {
    self.commands.lock().unwrap().clone()
  }
}

#[async_trait]
impl AgentClient for RecordingClient {
  async fn execute(&self, command: AgentCommand) -> Result<(), AgentClientError> {
    self.commands.lock().unwrap().push(RecordedCommand {
      expected_server_instance_id: None,
      command,
    });
    Ok(())
  }

  async fn execute_for_server(
    &self,
    expected_server_instance_id: &str,
    command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    self.commands.lock().unwrap().push(RecordedCommand {
      expected_server_instance_id: Some(expected_server_instance_id.to_owned()),
      command,
    });
    Ok(())
  }

  fn snapshot(&self) -> RuntimeSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.snapshots.clone()
  }
}

#[tokio::test]
async fn dispatcher_keeps_the_epoch_captured_before_the_task_is_polled() {
  let client = Arc::new(RecordingClient::new());
  let mut dispatcher = CommandDispatcher::new();
  dispatcher.dispatch_for_server(
    client.clone(),
    "daemon-a".into(),
    AgentCommand::StartTurn {
      text: "inspect".into(),
    },
  );

  assert!(client.commands().is_empty());
  tokio::task::yield_now().await;
  assert_eq!(
    client.commands(),
    [RecordedCommand {
      expected_server_instance_id: Some("daemon-a".into()),
      command: AgentCommand::StartTurn {
        text: "inspect".into()
      },
    }]
  );
}

#[tokio::test]
async fn turn_control_is_bound_to_the_epoch_that_reported_the_active_turn() {
  let client = RecordingClient::new();
  execute_control_for_server_with_timeout(
    &client,
    "daemon-a",
    AgentCommand::CancelTurn,
    Duration::from_millis(25),
  )
  .await
  .unwrap();

  assert_eq!(
    client.commands(),
    [RecordedCommand {
      expected_server_instance_id: Some("daemon-a".into()),
      command: AgentCommand::CancelTurn,
    }]
  );
}

#[test]
fn model_setup_requires_an_authoritative_remote_epoch() {
  let mut state = TuiState::default();
  state.sync_snapshot(&setup_snapshot());
  assert_eq!(state.accepted_server_instance_id(), None);
  assert!(!state.can_issue_setup_commands());

  apply_remote_state(&mut state, &connected("daemon-a", setup_snapshot()));
  assert_eq!(state.accepted_server_instance_id(), Some("daemon-a"));
  assert!(state.can_issue_setup_commands());
}

#[test]
fn a_generic_snapshot_is_read_only_without_an_authoritative_epoch() {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    ..RuntimeSnapshot::default()
  });

  assert_eq!(state.accepted_server_instance_id(), None);
  assert!(!state.can_submit());
}

#[test]
fn epoch_change_releases_closing_without_allowing_old_completion_to_clear_new_work() {
  let setup = setup_snapshot().model_setup.unwrap();
  let mut gate = ModelSetupCommandGate::default();
  assert!(!gate.observe_server_epoch("daemon-a"));
  let token_a = begin_setup(&mut gate, &setup);
  assert_eq!(gate.request_exit(), ModelSetupExit::WaitForSetup);

  assert!(gate.observe_server_epoch("daemon-b"));
  assert!(!gate.is_pending());
  assert!(!gate.is_closing());
  let token_b = begin_setup(&mut gate, &setup);
  assert!(token_b > token_a);

  assert_eq!(gate.complete(token_a, true), ModelSetupCompletion::Continue);
  assert_eq!(gate.pending_token(), Some(token_b));
}

#[test]
fn new_authoritative_epoch_clears_old_command_error() {
  let mut state = TuiState::default();
  apply_remote_state(&mut state, &connected("daemon-a", setup_snapshot()));
  state.record_client_failure(AgentClientError::unavailable(
    AvailabilityKind::StaleEpoch,
    "old\u{1b}[31m daemon",
  ));
  assert_eq!(
    state.command_error(),
    Some("agent client unavailable (stale_epoch): old daemon")
  );
  assert_eq!(
    state.connection_error(),
    Some("agent client unavailable (stale_epoch): old daemon")
  );

  apply_remote_state(&mut state, &connected("daemon-b", setup_snapshot()));
  assert_eq!(state.command_error(), None);
}

fn begin_setup(gate: &mut ModelSetupCommandGate, setup: &ModelSetupView) -> u64 {
  assert!(matches!(
    gate.input(
      setup,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(_)
  ));
  gate.pending_token().unwrap()
}

fn connected(server_instance_id: &str, snapshot: RuntimeSnapshot) -> RemoteState {
  RemoteState {
    connection: ConnectionState::Connected {
      server_instance_id: server_instance_id.into(),
    },
    snapshot: Some(VersionedRemoteSnapshot {
      server_instance_id: server_instance_id.into(),
      revision: 1,
      snapshot,
    }),
  }
}

fn setup_snapshot() -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(ModelSetupView {
      source: ModelSetupSource::ManagedPreset,
      phase: ModelSetupPhase::Required,
      managed_model: Some(ManagedModelView {
        id: "model-a".into(),
        expected_download_bytes: 1,
        max_bytes: 2,
        min_available_memory_bytes: 3,
        sha256: "digest".into(),
      }),
      operation_id: None,
      downloaded_bytes: 0,
      total_bytes: None,
      failure: None,
      allowed_actions: vec![ModelSetupAction::Download],
    }),
    ..RuntimeSnapshot::default()
  }
}
