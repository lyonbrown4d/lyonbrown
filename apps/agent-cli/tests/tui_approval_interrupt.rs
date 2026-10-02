use std::sync::{Arc, Mutex};

use agent_app::{
  AgentClient, AgentClientError, AgentCommand, ApprovalState, ApprovalView, AvailabilityKind,
  RuntimePhase, RuntimeSnapshot,
};
use agent_cli::tui::runner::{
  ApprovalInputOutcome, Runtime, handle_approval_interrupt, sync_runtime_snapshot,
};
use agent_cli::tui::{Locale, TuiState};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use agent_core::{RiskLevel, SandboxStatus, StepId, ToolCallId, ToolPreview};
use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

#[tokio::test]
async fn first_approval_interrupt_cancels_then_second_force_stops_and_exits() {
  let client = Arc::new(RecordingClient::new(false));
  let mut runtime = runtime(client.clone());
  let mut cancel_requested = false;

  let first = handle_approval_interrupt(&mut runtime, ctrl_c(), &mut cancel_requested)
    .await
    .unwrap();
  assert_eq!(first, ApprovalInputOutcome::Handled);
  assert!(cancel_requested);

  let second = handle_approval_interrupt(&mut runtime, ctrl_c(), &mut cancel_requested)
    .await
    .unwrap();
  assert_eq!(second, ApprovalInputOutcome::Exit);
  assert_eq!(
    client.commands(),
    [
      AgentCommand::RespondApproval {
        request_id: runtime.approval.as_ref().unwrap().request_id,
        decision: agent_app::ApprovalDecision::DenyOnce,
      },
      AgentCommand::CancelTurn,
      AgentCommand::ForceStop,
    ]
  );
  assert_eq!(client.epochs(), ["daemon-a", "daemon-a", "daemon-a"]);
}

#[tokio::test]
async fn deny_failure_with_successful_cancel_still_allows_second_force_stop() {
  let client = Arc::new(RecordingClient::new(true));
  let mut runtime = runtime(client.clone());
  let mut cancel_requested = false;

  let first = handle_approval_interrupt(&mut runtime, ctrl_c(), &mut cancel_requested)
    .await
    .unwrap();
  assert_eq!(first, ApprovalInputOutcome::Handled);
  assert!(cancel_requested);
  assert!(runtime.state.is_busy());
  assert!(runtime.state.command_error().is_none());
  assert!(
    runtime
      .state
      .client_local_notice()
      .is_some_and(|notice| notice.contains("deny approval"))
  );

  let second = handle_approval_interrupt(&mut runtime, ctrl_c(), &mut cancel_requested)
    .await
    .unwrap();
  assert_eq!(second, ApprovalInputOutcome::Exit);
  assert!(matches!(client.commands()[2], AgentCommand::ForceStop));
}

fn runtime(client: Arc<RecordingClient>) -> Runtime {
  let request_id = ToolCallId::new();
  let snapshot = approval_snapshot(request_id);
  let remote = connected("daemon-a", snapshot);
  let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
  let (_, remote_state) = watch::channel(remote);
  let mut runtime = Runtime {
    state: TuiState::default(),
    client: Some(client),
    snapshots,
    remote_state: Some(remote_state),
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
    locale: Locale::EnUs,
  };
  sync_runtime_snapshot(&mut runtime);
  runtime
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

fn approval_snapshot(request_id: ToolCallId) -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::AwaitingApproval,
    approval: Some(ApprovalView {
      request_id,
      step_id: StepId::new(),
      tool_name: "shell".into(),
      arguments: serde_json::json!({"command": "echo safe"}),
      preview: Some(ToolPreview {
        tool: "shell".into(),
        risk: RiskLevel::Execute,
        action: "execute command".into(),
        target: "echo safe".into(),
        impact: "prints text".into(),
        working_directory: None,
        sandbox: SandboxStatus::WorkspaceRestricted,
        required_secrets: vec![],
      }),
      deadline: None,
      state: ApprovalState::Pending,
      approved_by: None,
    }),
    ..RuntimeSnapshot::default()
  }
}

fn ctrl_c() -> KeyEvent {
  KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

struct RecordingClient {
  commands: Mutex<Vec<AgentCommand>>,
  epochs: Mutex<Vec<String>>,
  snapshots: watch::Sender<RuntimeSnapshot>,
  fail_deny: bool,
}

impl RecordingClient {
  fn new(fail_deny: bool) -> Self {
    let (snapshots, _) = watch::channel(RuntimeSnapshot::default());
    Self {
      commands: Mutex::new(Vec::new()),
      epochs: Mutex::new(Vec::new()),
      snapshots,
      fail_deny,
    }
  }

  fn commands(&self) -> Vec<AgentCommand> {
    self.commands.lock().unwrap().clone()
  }

  fn epochs(&self) -> Vec<String> {
    self.epochs.lock().unwrap().clone()
  }
}

#[async_trait]
impl AgentClient for RecordingClient {
  async fn execute(&self, _command: AgentCommand) -> Result<(), AgentClientError> {
    panic!("approval interrupt must remain epoch-bound")
  }

  async fn execute_for_server(
    &self,
    expected_server_instance_id: &str,
    command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    self
      .epochs
      .lock()
      .unwrap()
      .push(expected_server_instance_id.to_owned());
    let deny = matches!(command, AgentCommand::RespondApproval { .. });
    self.commands.lock().unwrap().push(command);
    if self.fail_deny && deny {
      return Err(AgentClientError::unavailable(
        AvailabilityKind::Disconnected,
        "deny failed",
      ));
    }
    Ok(())
  }

  fn snapshot(&self) -> RuntimeSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.snapshots.subscribe()
  }
}
