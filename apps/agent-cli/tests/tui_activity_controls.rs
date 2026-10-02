use agent_app::{
  ActivityPhase, ActivityView, ApprovalState, ApprovalView, RuntimePhase, RuntimeSnapshot,
  TurnStatus, TurnView,
};
use agent_cli::tui::runner::{Runtime, sync_runtime_snapshot};
use agent_cli::tui::{ActivityFocus, Locale, OverlayState, TuiState};
use agent_core::{RiskLevel, SandboxStatus, StepId, ToolCallId, ToolPreview, TurnId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

#[test]
fn inspector_switches_to_palette_and_control_c_closes_it() {
  let mut state = state_with_activity();
  state.open_activity();

  state.apply_overlay_key(key(KeyCode::Char('p'), KeyModifiers::CONTROL), 20);
  assert!(matches!(state.overlay(), OverlayState::Palette { .. }));

  state.open_activity();
  state.apply_overlay_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), 20);
  assert_eq!(state.overlay(), &OverlayState::Closed);
}

#[test]
fn detail_navigation_covers_scroll_bounds_and_both_list_shortcuts() {
  let mut state = state_with_activity();
  state.open_activity();
  state.apply_overlay_key(key(KeyCode::Right, KeyModifiers::NONE), 20);
  assert_focus(&state, ActivityFocus::Detail);

  state.apply_overlay_key(key(KeyCode::Down, KeyModifiers::NONE), 20);
  assert_eq!(detail_scroll(&state), 1);
  state.apply_overlay_key(key(KeyCode::PageDown, KeyModifiers::NONE), 20);
  assert!(detail_scroll(&state) > 1);
  let page_down = detail_scroll(&state);
  state.apply_overlay_key(key(KeyCode::PageUp, KeyModifiers::NONE), 20);
  assert!(detail_scroll(&state) < page_down);
  state.apply_overlay_key(key(KeyCode::End, KeyModifiers::NONE), 20);
  assert_eq!(detail_scroll(&state), u16::MAX);
  state.clamp_activity_detail_scroll(9);
  state.apply_overlay_key(key(KeyCode::Up, KeyModifiers::NONE), 20);
  assert_eq!(detail_scroll(&state), 8);
  state.apply_overlay_key(key(KeyCode::Home, KeyModifiers::NONE), 20);
  assert_eq!(detail_scroll(&state), 0);

  state.apply_overlay_key(key(KeyCode::Left, KeyModifiers::NONE), 20);
  assert_focus(&state, ActivityFocus::List);
  state.apply_overlay_key(key(KeyCode::Tab, KeyModifiers::NONE), 20);
  assert_focus(&state, ActivityFocus::Detail);
  state.apply_overlay_key(key(KeyCode::BackTab, KeyModifiers::SHIFT), 20);
  assert_focus(&state, ActivityFocus::List);
}

#[test]
fn approval_arrival_closes_activity_and_completion_never_restores_it() {
  let (sender, snapshots) = watch::channel(snapshot());
  let mut runtime = runtime(snapshots);
  sync_runtime_snapshot(&mut runtime);
  runtime.state.open_activity();

  let mut awaiting = snapshot();
  awaiting.phase = RuntimePhase::AwaitingApproval;
  awaiting.approval = Some(ApprovalView {
    request_id: ToolCallId::new(),
    step_id: StepId::new(),
    tool_name: "shell".into(),
    arguments: serde_json::json!({"command": "echo safe"}),
    preview: Some(ToolPreview {
      tool: "shell".into(),
      risk: RiskLevel::Execute,
      action: "execute".into(),
      target: "echo safe".into(),
      impact: "prints".into(),
      working_directory: None,
      sandbox: SandboxStatus::WorkspaceRestricted,
      required_secrets: vec![],
    }),
    deadline: None,
    state: ApprovalState::Pending,
    approved_by: None,
  });
  sender.send(awaiting).unwrap();
  assert!(runtime.snapshots.has_changed().unwrap());
  sync_runtime_snapshot(&mut runtime);
  assert_eq!(runtime.state.overlay(), &OverlayState::Closed);
  assert!(runtime.approval.is_some());

  sender.send(snapshot()).unwrap();
  sync_runtime_snapshot(&mut runtime);
  assert_eq!(runtime.state.overlay(), &OverlayState::Closed);
  assert!(runtime.approval.is_none());
}

fn assert_focus(state: &TuiState, expected: ActivityFocus) {
  let OverlayState::Activity(inspector) = state.overlay() else {
    panic!("activity inspector closed")
  };
  assert_eq!(inspector.focus, expected);
}

fn detail_scroll(state: &TuiState) -> u16 {
  let OverlayState::Activity(inspector) = state.overlay() else {
    panic!("activity inspector closed")
  };
  inspector.detail_scroll
}

fn state_with_activity() -> TuiState {
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot());
  state
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

fn snapshot() -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    turns: vec![TurnView {
      turn_id: TurnId::new(),
      user_text: "inspect".into(),
      steering: vec![],
      assistant_text: String::new(),
      status: TurnStatus::Running,
      activities: vec![ActivityView {
        step_id: StepId::new(),
        phase: ActivityPhase::Streaming,
        route: None,
        context: None,
        assistant_text: "wrapped ".repeat(200),
        usage: None,
        tools: vec![],
        outcome: None,
      }],
    }],
    ..RuntimeSnapshot::default()
  }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
  KeyEvent::new(code, modifiers)
}
