use std::sync::{Arc, Mutex};

use agent_app::{
  AgentClient, AgentClientError, AgentCommand, AvailabilityKind, ManagedModelView,
  ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView, RuntimeSnapshot,
};
use agent_cli::tui::{
  TuiState,
  runner::{
    CommandDispatcher, ModelSetupCommandGate, ModelSetupCompletion, ModelSetupExit, ModelSetupInput,
  },
};
use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

#[derive(Clone, Copy)]
enum ExecuteOutcome {
  Accepted,
  Unavailable,
  Panic,
}

struct FakeClient {
  commands: Mutex<Vec<AgentCommand>>,
  expected_server_instance_ids: Mutex<Vec<String>>,
  snapshots: watch::Receiver<RuntimeSnapshot>,
  outcome: ExecuteOutcome,
}

impl FakeClient {
  fn new(outcome: ExecuteOutcome) -> Self {
    let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
    Self {
      commands: Mutex::new(Vec::new()),
      expected_server_instance_ids: Mutex::new(Vec::new()),
      snapshots,
      outcome,
    }
  }

  fn commands(&self) -> Vec<AgentCommand> {
    self.commands.lock().unwrap().clone()
  }

  fn expected_server_instance_ids(&self) -> Vec<String> {
    self.expected_server_instance_ids.lock().unwrap().clone()
  }
}

#[async_trait]
impl AgentClient for FakeClient {
  async fn execute(&self, _command: AgentCommand) -> Result<(), AgentClientError> {
    panic!("model setup commands must never use the unbound execute path")
  }

  async fn execute_for_server(
    &self,
    expected_server_instance_id: &str,
    command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    self
      .expected_server_instance_ids
      .lock()
      .unwrap()
      .push(expected_server_instance_id.to_owned());
    self.commands.lock().unwrap().push(command);
    match self.outcome {
      ExecuteOutcome::Accepted => Ok(()),
      ExecuteOutcome::Unavailable => Err(AgentClientError::unavailable(
        AvailabilityKind::Disconnected,
        "daemon offline",
      )),
      ExecuteOutcome::Panic => panic!("simulated setup RPC panic"),
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
async fn setup_dispatcher_reports_success_and_failure_completions() {
  for outcome in [ExecuteOutcome::Accepted, ExecuteOutcome::Unavailable] {
    let client = Arc::new(FakeClient::new(outcome));
    let mut dispatcher = CommandDispatcher::new();
    dispatcher.dispatch_model_setup(
      client,
      "daemon-a".into(),
      AgentCommand::StartModelDownload {
        model_id: "model-a".into(),
      },
      1,
    );
    tokio::task::yield_now().await;

    let completions = dispatcher.drain_completions();
    assert_eq!(completions.len(), 1);
    assert!(completions[0].is_model_setup());
    assert_eq!(
      completions[0].result().is_ok(),
      matches!(outcome, ExecuteOutcome::Accepted)
    );
  }
}

#[tokio::test]
async fn panicking_setup_task_keeps_its_origin_and_completes() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::Panic));
  let mut dispatcher = CommandDispatcher::new();
  dispatcher.dispatch_model_setup(
    client,
    "daemon-a".into(),
    AgentCommand::StartModelDownload {
      model_id: "model-a".into(),
    },
    7,
  );
  tokio::task::yield_now().await;

  let completion = dispatcher.drain_completions().pop().unwrap();
  assert!(completion.is_model_setup());
  assert!(completion.is_task_failure());
  assert!(completion.result().is_err());
}

#[tokio::test]
async fn setup_exit_requested_before_the_rpc_runs_waits_for_accepted_receipt() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::Accepted));
  let mut dispatcher = CommandDispatcher::new();
  let mut gate = pending_setup_gate();
  let token = gate.pending_token().unwrap();
  dispatcher.dispatch_model_setup(
    client.clone(),
    "daemon-a".into(),
    AgentCommand::StartModelDownload {
      model_id: "model-a".into(),
    },
    token,
  );

  assert_eq!(gate.request_exit(), ModelSetupExit::WaitForSetup);
  assert!(client.commands().is_empty());
  tokio::task::yield_now().await;
  let completion = dispatcher.drain_completions().pop().unwrap();
  assert_eq!(completion.model_setup_token(), Some(token));
  assert_eq!(
    gate.complete(token, completion.result().is_ok()),
    ModelSetupCompletion::Exit
  );
  assert_eq!(
    client.commands(),
    [AgentCommand::StartModelDownload {
      model_id: "model-a".into()
    }]
  );
  assert_eq!(client.expected_server_instance_ids(), ["daemon-a"]);
}

#[tokio::test]
async fn failed_setup_receipt_leaves_the_screen_open_with_the_error() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::Unavailable));
  let mut dispatcher = CommandDispatcher::new();
  let mut gate = pending_setup_gate();
  let token = gate.pending_token().unwrap();
  dispatcher.dispatch_model_setup(
    client,
    "daemon-a".into(),
    AgentCommand::StartModelDownload {
      model_id: "model-a".into(),
    },
    token,
  );
  assert_eq!(gate.request_exit(), ModelSetupExit::WaitForSetup);

  tokio::task::yield_now().await;
  let completion = dispatcher.drain_completions().pop().unwrap();
  assert_eq!(
    gate.complete(token, completion.result().is_ok()),
    ModelSetupCompletion::Continue
  );
  let mut state = TuiState::default();
  state.record_client_failure(completion.into_result().unwrap_err());
  assert!(!gate.is_closing());
  assert!(state.command_error().unwrap().contains("daemon offline"));
}

#[tokio::test]
async fn ordinary_command_receipt_does_not_release_setup_closing_latch() {
  let client = Arc::new(FakeClient::new(ExecuteOutcome::Accepted));
  let mut dispatcher = CommandDispatcher::new();
  let mut gate = pending_setup_gate();
  assert_eq!(gate.request_exit(), ModelSetupExit::WaitForSetup);
  dispatcher.dispatch_for_server(
    client,
    "daemon-a".into(),
    AgentCommand::QueueFollowUp {
      text: "later".into(),
    },
  );

  tokio::task::yield_now().await;
  let completion = dispatcher.drain_completions().pop().unwrap();
  assert!(!completion.is_model_setup());
  assert!(gate.is_closing());
}

fn pending_setup_gate() -> ModelSetupCommandGate {
  let setup = ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase: ModelSetupPhase::Required,
    managed_model: Some(ManagedModelView {
      id: "model-a".into(),
      expected_download_bytes: 1024,
      max_bytes: 2048,
      min_available_memory_bytes: 4096,
      sha256: "abc123".into(),
    }),
    operation_id: None,
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions: vec![ModelSetupAction::Download],
  };
  let mut gate = ModelSetupCommandGate::default();
  assert!(matches!(
    gate.input(
      &setup,
      true,
      KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    ),
    ModelSetupInput::Dispatch(_)
  ));
  gate
}
