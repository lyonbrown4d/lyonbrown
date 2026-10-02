use std::sync::Arc;

use agent_app::{
  AgentClient, AgentClientError, AgentCommand, AvailabilityKind, ManagedModelView,
  ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView, RuntimePhase,
  RuntimeSnapshot,
};
use agent_cli::tui::{
  TuiConnectionState, TuiState, apply_remote_state,
  runner::{CommandDispatcher, ModelSetupCommandGate, ModelSetupInput, apply_command_completion},
};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

struct FailingClient {
  snapshots: watch::Receiver<RuntimeSnapshot>,
}

impl FailingClient {
  fn new() -> Self {
    let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
    Self { snapshots }
  }
}

#[async_trait]
impl AgentClient for FailingClient {
  async fn execute(&self, _command: AgentCommand) -> Result<(), AgentClientError> {
    panic!("TUI commands must use the epoch-bound path")
  }

  async fn execute_for_server(
    &self,
    _expected_server_instance_id: &str,
    _command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    Err(AgentClientError::unavailable(
      AvailabilityKind::Disconnected,
      "daemon-a went away",
    ))
  }

  fn snapshot(&self) -> RuntimeSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.snapshots.clone()
  }
}

#[tokio::test]
async fn delayed_ordinary_failure_from_epoch_a_is_ignored_after_epoch_b_is_accepted() {
  let mut state = TuiState::default();
  apply_remote_state(&mut state, &connected("daemon-a", ready_snapshot()));
  let mut dispatcher = CommandDispatcher::new();
  dispatcher.dispatch_for_server(
    Arc::new(FailingClient::new()),
    "daemon-a".into(),
    AgentCommand::QueueFollowUp {
      text: "later".into(),
    },
  );

  apply_remote_state(&mut state, &connected("daemon-b", ready_snapshot()));
  tokio::task::yield_now().await;
  let completion = dispatcher.drain_completions().pop().unwrap();
  let mut gate = ModelSetupCommandGate::default();
  apply_command_completion(&mut state, &mut gate, completion);

  assert_clean_epoch_b(&state);
}

#[tokio::test]
async fn delayed_setup_failure_from_epoch_a_is_ignored_after_epoch_b_is_accepted() {
  let snapshot = setup_snapshot();
  let setup = snapshot.model_setup.as_ref().unwrap();
  let mut state = TuiState::default();
  apply_remote_state(&mut state, &connected("daemon-a", snapshot.clone()));
  let mut gate = ModelSetupCommandGate::default();
  gate.observe_server_epoch("daemon-a");
  let token_a = begin_setup(&mut gate, setup);
  let mut dispatcher = CommandDispatcher::new();
  dispatcher.dispatch_model_setup(
    Arc::new(FailingClient::new()),
    "daemon-a".into(),
    AgentCommand::StartModelDownload {
      model_id: "model-a".into(),
    },
    token_a,
  );

  apply_remote_state(&mut state, &connected("daemon-b", snapshot));
  gate.observe_server_epoch("daemon-b");
  let token_b = begin_setup(&mut gate, state.model_setup().unwrap());
  tokio::task::yield_now().await;
  let completion = dispatcher.drain_completions().pop().unwrap();
  apply_command_completion(&mut state, &mut gate, completion);

  assert_clean_epoch_b(&state);
  assert_eq!(gate.pending_token(), Some(token_b));
}

fn assert_clean_epoch_b(state: &TuiState) {
  assert_eq!(state.accepted_server_instance_id(), Some("daemon-b"));
  assert_eq!(state.connection(), TuiConnectionState::Connected);
  assert_eq!(state.connection_error(), None);
  assert_eq!(state.command_error(), None);
  assert!(state.transcript().is_empty());
}

fn begin_setup(gate: &mut ModelSetupCommandGate, setup: &ModelSetupView) -> u64 {
  assert!(matches!(
    gate.input(
      setup,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
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

fn ready_snapshot() -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    ..RuntimeSnapshot::default()
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
