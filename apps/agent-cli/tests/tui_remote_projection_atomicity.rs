use agent_app::{
  ActivityPhase, ActivityView, ApprovalState, ApprovalView, RuntimePhase, RuntimeSnapshot,
  TurnStatus, TurnView,
};
use agent_cli::tui::runner::{Runtime, sync_runtime_snapshot};
use agent_cli::tui::{Locale, OverlayState, RenderContext, TuiState, render_with_context};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use agent_core::{RiskLevel, SandboxStatus, StepId, ToolCallId, ToolPreview, TurnId};
use ratatui::{Terminal, backend::TestBackend};
use tokio::sync::watch;

#[test]
fn same_revision_changed_payload_cannot_mutate_runtime_projections() {
  let initial = snapshot("accepted-queue");
  let (sender, remote_state) = watch::channel(remote("daemon-a", "daemon-a", 1, initial));
  let mut runtime = runtime(remote_state);
  sync_runtime_snapshot(&mut runtime);
  runtime.state.open_activity();

  let mut rejected = snapshot("rejected-queue");
  rejected.approval = Some(approval());
  sender
    .send(remote("daemon-a", "daemon-a", 1, rejected))
    .unwrap();
  sync_runtime_snapshot(&mut runtime);

  assert_rejected_projection(&mut runtime);
}

#[test]
fn mismatched_snapshot_epoch_cannot_mutate_runtime_projections() {
  let initial = snapshot("accepted-queue");
  let (sender, remote_state) = watch::channel(remote("daemon-a", "daemon-a", 1, initial));
  let mut runtime = runtime(remote_state);
  sync_runtime_snapshot(&mut runtime);
  runtime.state.open_activity();

  let mut rejected = snapshot("rejected-queue");
  rejected.approval = Some(approval());
  sender
    .send(remote("daemon-a", "daemon-b", 2, rejected))
    .unwrap();
  sync_runtime_snapshot(&mut runtime);

  assert_rejected_projection(&mut runtime);
}

fn assert_rejected_projection(runtime: &mut Runtime) {
  assert!(matches!(runtime.state.overlay(), OverlayState::Activity(_)));
  assert!(runtime.approval.is_none());
  runtime.state.close_overlay();
  let output = screen(&runtime.state);
  assert!(output.contains("accepted-queue"), "{output}");
  assert!(!output.contains("rejected-queue"), "{output}");
}

fn runtime(remote_state: watch::Receiver<RemoteState>) -> Runtime {
  let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
  Runtime {
    state: TuiState::default(),
    client: None,
    snapshots,
    remote_state: Some(remote_state),
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
    locale: Locale::EnUs,
  }
}

fn remote(
  connection_epoch: &str,
  snapshot_epoch: &str,
  revision: u64,
  snapshot: RuntimeSnapshot,
) -> RemoteState {
  RemoteState {
    connection: ConnectionState::Connected {
      server_instance_id: connection_epoch.into(),
    },
    snapshot: Some(VersionedRemoteSnapshot {
      server_instance_id: snapshot_epoch.into(),
      revision,
      snapshot,
    }),
  }
}

fn snapshot(queue: &str) -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    queued_follow_ups: vec![queue.into()],
    turns: vec![TurnView {
      turn_id: TurnId::new(),
      user_text: "task".into(),
      steering: vec![],
      assistant_text: String::new(),
      status: TurnStatus::Running,
      activities: vec![ActivityView {
        step_id: StepId::new(),
        phase: ActivityPhase::Completed,
        route: None,
        context: None,
        assistant_text: "accepted activity".into(),
        usage: None,
        tools: vec![],
        outcome: None,
      }],
    }],
    ..RuntimeSnapshot::default()
  }
}

fn approval() -> ApprovalView {
  ApprovalView {
    request_id: ToolCallId::new(),
    step_id: StepId::new(),
    tool_name: "shell".into(),
    arguments: serde_json::json!({"command": "rejected"}),
    preview: Some(ToolPreview {
      tool: "shell".into(),
      risk: RiskLevel::Execute,
      action: "execute".into(),
      target: "rejected".into(),
      impact: "none".into(),
      working_directory: None,
      sandbox: SandboxStatus::WorkspaceRestricted,
      required_secrets: vec![],
    }),
    deadline: None,
    state: ApprovalState::Pending,
    approved_by: None,
  }
}

fn screen(state: &TuiState) -> String {
  let backend = TestBackend::new(100, 28);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| {
      render_with_context(
        frame,
        state,
        RenderContext {
          locale: Locale::EnUs,
          ..RenderContext::default()
        },
      )
    })
    .unwrap();
  let buffer = terminal.backend().buffer();
  (0..buffer.area.height)
    .map(|y| {
      (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}
