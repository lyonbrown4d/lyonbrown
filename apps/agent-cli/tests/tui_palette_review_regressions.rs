use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_app::{
  AgentClient, AgentClientError, AgentCommand, ApprovalState, ApprovalView, AvailabilityKind,
  RuntimePhase, RuntimeSnapshot,
};
use agent_cli::tui::runner::{
  Runtime, execute_approval_interrupt_for_server_with_timeout, sync_runtime_snapshot,
};
use agent_cli::tui::{CommandId, Locale, OverlayState, TuiState, command_catalog, slash_command};
use agent_core::{RiskLevel, SandboxStatus, StepId, ToolCallId, ToolPreview};
use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

#[test]
fn catalog_is_the_exact_stable_m12_command_set_with_unique_aliases() {
  let expected = [
    "/model",
    "/theme",
    "/language",
    "/sessions",
    "/new",
    "/resume",
    "/status",
    "/queue",
    "/help",
    "/quit",
  ];
  let aliases = command_catalog()
    .iter()
    .map(|command| command.alias)
    .collect::<Vec<_>>();
  let unique = aliases.iter().copied().collect::<HashSet<_>>();

  assert_eq!(aliases, expected);
  assert_eq!(unique.len(), expected.len());
}

#[test]
fn every_catalog_entry_round_trips_through_the_unified_parser() {
  for command in command_catalog() {
    let input = match command.id {
      CommandId::Language => "/language zh-CN".to_owned(),
      CommandId::Queue => "/queue inspect tests".to_owned(),
      _ => command.alias.to_owned(),
    };
    assert_eq!(
      slash_command(&input).command_id(),
      Some(command.id),
      "{input}"
    );
  }
  assert!(slash_command("/models").command_id().is_none());
  assert!(slash_command("/activity").command_id().is_none());
}

#[test]
fn command_capabilities_match_implemented_local_and_daemon_actions() {
  let implemented = command_catalog()
    .iter()
    .filter(|command| command.implemented)
    .map(|command| command.id)
    .collect::<Vec<_>>();
  let daemon_backed = command_catalog()
    .iter()
    .filter(|command| command.daemon_backed)
    .map(|command| command.id)
    .collect::<Vec<_>>();

  assert_eq!(
    implemented,
    [
      CommandId::Language,
      CommandId::Queue,
      CommandId::Help,
      CommandId::Quit,
    ]
  );
  assert_eq!(
    daemon_backed,
    [
      CommandId::Model,
      CommandId::Sessions,
      CommandId::New,
      CommandId::Resume,
      CommandId::Status,
      CommandId::Queue,
    ]
  );
}

#[test]
fn approval_arrival_closes_overlay_and_completion_does_not_restore_it() {
  let (sender, snapshots) = watch::channel(RuntimeSnapshot::default());
  let mut runtime = runtime(snapshots);
  runtime.state.open_palette();
  sender.send(approval_snapshot()).unwrap();

  sync_runtime_snapshot(&mut runtime);

  assert_eq!(runtime.state.overlay(), &OverlayState::Closed);
  assert!(runtime.approval.is_some());
  sender.send(RuntimeSnapshot::default()).unwrap();
  sync_runtime_snapshot(&mut runtime);
  assert_eq!(runtime.state.overlay(), &OverlayState::Closed);
  assert!(runtime.approval.is_none());
}

#[tokio::test]
async fn approval_interrupt_sends_deny_once_before_cancel_turn() {
  let client = Arc::new(RecordingClient::new(false));
  let request_id = ToolCallId::new();

  let outcome = execute_approval_interrupt_for_server_with_timeout(
    client.as_ref(),
    "daemon-a",
    request_id,
    Duration::from_millis(25),
  )
  .await;

  assert!(outcome.failure_notice().is_none());
  assert!(outcome.cancel_succeeded());

  assert_eq!(
    client.commands(),
    [
      AgentCommand::RespondApproval {
        request_id,
        decision: agent_app::ApprovalDecision::DenyOnce,
      },
      AgentCommand::CancelTurn,
    ]
  );
}

#[tokio::test]
async fn approval_interrupt_still_cancels_when_deny_receipt_fails() {
  let client = Arc::new(RecordingClient::new(true));
  let request_id = ToolCallId::new();

  let outcome = execute_approval_interrupt_for_server_with_timeout(
    client.as_ref(),
    "daemon-a",
    request_id,
    Duration::from_millis(25),
  )
  .await;

  assert!(
    outcome
      .failure_notice()
      .is_some_and(|notice| notice.contains("deny approval"))
  );
  assert!(outcome.cancel_succeeded());
  assert_eq!(client.commands().len(), 2);
  assert!(matches!(client.commands()[1], AgentCommand::CancelTurn));
}

#[test]
fn compact_help_scroll_is_clamped_for_all_navigation_keys() {
  let mut state = TuiState::default();
  state.open_help();
  let key = |code| KeyEvent::new(code, KeyModifiers::NONE);

  state.apply_overlay_key(key(KeyCode::Up), 8);
  assert_eq!(help_scroll(&state), 0);
  state.apply_overlay_key(key(KeyCode::End), 8);
  let end = help_scroll(&state);
  assert!(end > 0);
  state.apply_overlay_key(key(KeyCode::Down), 8);
  assert_eq!(help_scroll(&state), end);
  state.apply_overlay_key(key(KeyCode::PageUp), 8);
  assert_eq!(help_scroll(&state), end.saturating_sub(5));
  state.apply_overlay_key(key(KeyCode::PageDown), 8);
  assert_eq!(help_scroll(&state), end);
  state.apply_overlay_key(key(KeyCode::Home), 8);
  assert_eq!(help_scroll(&state), 0);
}

fn runtime(snapshots: watch::Receiver<RuntimeSnapshot>) -> Runtime {
  Runtime {
    state: TuiState::default(),
    client: None,
    snapshots,
    remote_state: None,
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
    locale: Locale::EnUs,
  }
}

fn approval_snapshot() -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::AwaitingApproval,
    approval: Some(ApprovalView {
      request_id: ToolCallId::new(),
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

fn help_scroll(state: &TuiState) -> u16 {
  match state.overlay() {
    OverlayState::Help { scroll } => *scroll,
    overlay => panic!("expected help overlay, got {overlay:?}"),
  }
}

struct RecordingClient {
  commands: Mutex<Vec<AgentCommand>>,
  snapshots: watch::Sender<RuntimeSnapshot>,
  fail_first: bool,
}

impl RecordingClient {
  fn new(fail_first: bool) -> Self {
    let (snapshots, _) = watch::channel(RuntimeSnapshot::default());
    Self {
      commands: Mutex::new(Vec::new()),
      snapshots,
      fail_first,
    }
  }

  fn commands(&self) -> Vec<AgentCommand> {
    self.commands.lock().unwrap().clone()
  }
}

#[async_trait]
impl AgentClient for RecordingClient {
  async fn execute(&self, _command: AgentCommand) -> Result<(), AgentClientError> {
    panic!("approval interrupt must remain epoch-bound")
  }

  async fn execute_for_server(
    &self,
    _expected_server_instance_id: &str,
    command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    let mut commands = self.commands.lock().unwrap();
    commands.push(command);
    if self.fail_first && commands.len() == 1 {
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
