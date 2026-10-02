use agent_app::{
  ActivityPhase, ActivityView, ApprovalState, ApprovalView, RuntimePhase, RuntimeSnapshot,
  ToolActivityState, ToolActivityView, TurnStatus, TurnView,
};
use agent_cli::tui::runner::{Runtime, sync_runtime_snapshot};
use agent_cli::tui::{Locale, OverlayState, TuiState, apply_remote_state};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use agent_core::{RiskLevel, SandboxStatus, StepId, ToolCallId, ToolPreview, TurnId};
use tokio::sync::watch;

#[test]
fn unchanged_local_watch_value_is_not_reprojected() {
  let (sender, snapshots) = watch::channel(snapshot("first", 1));
  let mut runtime = runtime(snapshots);
  sync_runtime_snapshot(&mut runtime);
  let first_generation = runtime.state.activity_projection_generation();

  sync_runtime_snapshot(&mut runtime);
  assert_eq!(
    runtime.state.activity_projection_generation(),
    first_generation
  );

  sender.send(snapshot("second", 1)).unwrap();
  sync_runtime_snapshot(&mut runtime);
  assert!(runtime.state.activity_projection_generation() > first_generation);
}

#[test]
fn closed_local_watch_does_not_trigger_a_busy_reprojection_loop() {
  let (sender, snapshots) = watch::channel(snapshot("first", 1));
  let mut runtime = runtime(snapshots);
  sync_runtime_snapshot(&mut runtime);
  let generation = runtime.state.activity_projection_generation();
  drop(sender);

  sync_runtime_snapshot(&mut runtime);
  sync_runtime_snapshot(&mut runtime);

  assert_eq!(runtime.state.activity_projection_generation(), generation);
}

#[test]
fn local_watch_consumes_the_final_unseen_value_after_sender_closes() {
  let (sender, snapshots) = watch::channel(snapshot("first", 1));
  let mut runtime = runtime(snapshots);
  sync_runtime_snapshot(&mut runtime);

  sender.send(snapshot("final-local", 2)).unwrap();
  drop(sender);
  sync_runtime_snapshot(&mut runtime);

  runtime.state.open_activity();
  assert!(
    runtime
      .state
      .activity_detail(Locale::EnUs)
      .join("\n")
      .contains("final-local")
  );
  assert_eq!(runtime.state.activities().len(), 2);
}

#[test]
fn remote_watch_consumes_the_final_unseen_value_after_sender_closes() {
  let (sender, remote_state) = watch::channel(remote(1, snapshot("first", 1)));
  let mut runtime = remote_runtime(TuiState::default(), remote_state);
  sync_runtime_snapshot(&mut runtime);

  sender.send(remote(2, snapshot("final-remote", 2))).unwrap();
  drop(sender);
  sync_runtime_snapshot(&mut runtime);

  runtime.state.open_activity();
  assert!(
    runtime
      .state
      .activity_detail(Locale::EnUs)
      .join("\n")
      .contains("final-remote")
  );
  assert_eq!(runtime.state.activities().len(), 2);
}

#[test]
fn closed_remote_watch_does_not_repeat_an_already_consumed_projection() {
  let (sender, remote_state) = watch::channel(remote(1, snapshot("first", 1)));
  let mut runtime = remote_runtime(TuiState::default(), remote_state);
  sync_runtime_snapshot(&mut runtime);
  let generation = runtime.state.activity_projection_generation();
  drop(sender);

  sync_runtime_snapshot(&mut runtime);
  sync_runtime_snapshot(&mut runtime);

  assert_eq!(runtime.state.activity_projection_generation(), generation);
}

#[test]
fn repeated_remote_revision_never_reprojects_even_if_payload_differs() {
  let mut state = TuiState::default();
  apply_remote_state(&mut state, &remote(7, snapshot("first", 1)));
  let generation = state.activity_projection_generation();

  apply_remote_state(&mut state, &remote(7, snapshot("wrong", 2)));

  assert_eq!(state.activity_projection_generation(), generation);
  assert_eq!(state.activities().len(), 1);
  state.open_activity();
  assert!(
    state
      .activity_detail(Locale::EnUs)
      .join("\n")
      .contains("first")
  );

  apply_remote_state(&mut state, &remote(8, snapshot("new", 2)));
  assert!(state.activity_projection_generation() > generation);
  assert_eq!(state.activities().len(), 2);
}

#[test]
fn selected_detail_is_cached_until_its_key_changes() {
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot("cached", 2));
  state.open_activity();
  assert_eq!(state.activity_detail_format_count(), 0);

  let _ = state.activity_detail(Locale::EnUs);
  let _ = state.activity_detail(Locale::EnUs);
  assert_eq!(state.activity_detail_format_count(), 1);

  let _ = state.activity_detail(Locale::ZhCn);
  assert_eq!(state.activity_detail_format_count(), 2);
  state.apply_overlay_key(
    crossterm::event::KeyEvent::new(
      crossterm::event::KeyCode::Up,
      crossterm::event::KeyModifiers::NONE,
    ),
    20,
  );
  let _ = state.activity_detail(Locale::ZhCn);
  assert_eq!(state.activity_detail_format_count(), 3);

  state.sync_snapshot(&snapshot("changed", 2));
  let _ = state.activity_detail(Locale::ZhCn);
  assert_eq!(state.activity_detail_format_count(), 4);
}

#[test]
fn detail_has_bounded_tools_lines_and_json_work() {
  let call_id = ToolCallId::new();
  let tools = (0..80)
    .map(|index| ToolActivityView {
      call_id,
      name: format!("tool-{index}"),
      arguments: serde_json::json!({
        "nested": ["x".repeat(200_000), {"more": "y".repeat(200_000)}]
      }),
      state: ToolActivityState::Completed,
      approved_by: None,
      result: None,
    })
    .collect();
  let mut value = snapshot("bounded", 1);
  value.turns[0].activities[0].tools = tools;
  let mut state = TuiState::default();
  state.sync_snapshot(&value);
  state.open_activity();

  let detail = state.activity_detail(Locale::EnUs);
  let rendered = detail.join("\n");

  assert!(detail.len() <= 128, "{} lines", detail.len());
  assert!(rendered.len() <= 16_500, "{} bytes", rendered.len());
  assert!(rendered.contains("details truncated"), "{rendered}");
  assert_eq!(
    detail.last().map(String::as_str),
    Some("… details truncated")
  );
}

#[test]
fn unchanged_preprojected_remote_state_does_not_resync_approval() {
  let mut initial = snapshot("approval", 1);
  initial.phase = RuntimePhase::AwaitingApproval;
  initial.approval = Some(ApprovalView {
    request_id: ToolCallId::new(),
    step_id: StepId::new(),
    tool_name: "shell".into(),
    arguments: serde_json::json!({}),
    preview: Some(ToolPreview {
      tool: "shell".into(),
      risk: RiskLevel::Execute,
      action: "execute".into(),
      target: "safe".into(),
      impact: "none".into(),
      working_directory: None,
      sandbox: SandboxStatus::WorkspaceRestricted,
      required_secrets: vec![],
    }),
    deadline: None,
    state: ApprovalState::Pending,
    approved_by: None,
  });
  let remote = remote(1, initial);
  let mut state = TuiState::default();
  apply_remote_state(&mut state, &remote);
  state.open_activity();
  let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
  let (_, remote_state) = watch::channel(remote);
  let mut runtime = Runtime {
    state,
    client: None,
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

  assert!(runtime.approval.is_none());
  assert!(matches!(runtime.state.overlay(), OverlayState::Activity(_)));
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

fn remote_runtime(state: TuiState, remote_state: watch::Receiver<RemoteState>) -> Runtime {
  let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
  Runtime {
    state,
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

fn remote(revision: u64, snapshot: RuntimeSnapshot) -> RemoteState {
  RemoteState {
    connection: ConnectionState::Connected {
      server_instance_id: "daemon-a".into(),
    },
    snapshot: Some(VersionedRemoteSnapshot {
      server_instance_id: "daemon-a".into(),
      revision,
      snapshot,
    }),
  }
}

fn snapshot(text: &str, count: usize) -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    turns: vec![TurnView {
      turn_id: TurnId::new(),
      user_text: "task".into(),
      steering: vec![],
      assistant_text: String::new(),
      status: TurnStatus::Running,
      activities: (0..count)
        .map(|_| ActivityView {
          step_id: StepId::new(),
          phase: ActivityPhase::Streaming,
          route: None,
          context: None,
          assistant_text: text.into(),
          usage: None,
          tools: vec![],
          outcome: None,
        })
        .collect(),
    }],
    ..RuntimeSnapshot::default()
  }
}
