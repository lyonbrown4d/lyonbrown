use agent_app::{
  ActivityPhase, ActivityView, ModelSetupPhase, ModelSetupSource, ModelSetupView, RuntimePhase,
  RuntimeSnapshot, TurnStatus, TurnView,
};
use agent_cli::tui::runner::{LanguageCommand, Runtime};
use agent_cli::tui::{
  ActivityFocus, ActivityTarget, Locale, OverlayState, TuiState, apply_remote_state,
  command_catalog, slash_command,
};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use agent_core::{SessionId, StepId, TurnId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::watch;

#[test]
fn ctrl_o_opens_latest_and_toggles_from_other_overlays() {
  let (mut state, targets) = state_with_steps(3);
  let ctrl_o = key(KeyCode::Char('o'), KeyModifiers::CONTROL);

  state.open_palette();
  state.apply_overlay_key(ctrl_o, 20);
  assert_activity(&state, targets[2], ActivityFocus::List, true);
  state.apply_overlay_key(ctrl_o, 20);
  assert_eq!(state.overlay(), &OverlayState::Closed);

  state.open_help();
  state.apply_overlay_key(ctrl_o, 20);
  assert_activity(&state, targets[2], ActivityFocus::List, true);
  state.apply_overlay_key(key(KeyCode::Esc, KeyModifiers::NONE), 20);
  assert_eq!(state.overlay(), &OverlayState::Closed);
}

#[test]
fn list_navigation_preserves_target_across_snapshot_updates() {
  let (mut state, targets) = state_with_steps(4);
  state.open_activity();

  state.apply_overlay_key(key(KeyCode::Up, KeyModifiers::NONE), 20);
  assert_activity(&state, targets[2], ActivityFocus::List, false);
  let mut updated = snapshot_with_steps(5);
  updated.turns[0].turn_id = targets[2].turn_id;
  updated.turns[0].activities[2].step_id = targets[2].step_id;
  state.sync_snapshot(&updated);

  assert_activity(&state, targets[2], ActivityFocus::List, false);
}

#[test]
fn missing_target_falls_back_to_latest_and_resets_detail_scroll() {
  let (mut state, _) = state_with_steps(3);
  state.open_activity();
  state.apply_overlay_key(key(KeyCode::Enter, KeyModifiers::NONE), 20);
  state.apply_overlay_key(key(KeyCode::End, KeyModifiers::NONE), 20);
  state.sync_snapshot(&snapshot_with_steps(1));

  let OverlayState::Activity(inspector) = state.overlay() else {
    panic!("activity inspector closed")
  };
  assert_eq!(
    inspector.selected,
    state.activities().last().map(|item| item.target)
  );
  assert_eq!(inspector.detail_scroll, 0);
  assert!(inspector.follow_latest);
}

#[test]
fn session_or_daemon_epoch_change_closes_and_clears_old_selection() {
  let session_a = SessionId::new();
  let session_b = SessionId::new();
  let mut state = TuiState::default();
  let mut first = snapshot_with_steps(2);
  first.session_id = Some(session_a);
  apply_remote_state(&mut state, &connected("daemon-a", first));
  state.open_activity();

  let mut next_session = snapshot_with_steps(1);
  next_session.session_id = Some(session_b);
  apply_remote_state(&mut state, &connected_revision("daemon-a", 2, next_session));
  assert_eq!(state.overlay(), &OverlayState::Closed);

  state.open_activity();
  apply_remote_state(&mut state, &connected("daemon-b", snapshot_with_steps(1)));
  assert_eq!(state.overlay(), &OverlayState::Closed);
}

#[test]
fn assigning_a_session_after_an_unscoped_snapshot_is_a_session_change() {
  let (mut state, _) = state_with_steps(1);
  state.open_activity();
  let mut scoped = snapshot_with_steps(1);
  scoped.session_id = Some(SessionId::new());

  state.sync_snapshot(&scoped);

  assert_eq!(state.overlay(), &OverlayState::Closed);
}

#[test]
fn exact_control_o_does_not_change_the_fixed_command_catalog() {
  let (mut state, _) = state_with_steps(1);
  state.apply_overlay_key(key(KeyCode::Char('o'), KeyModifiers::CONTROL), 20);
  assert!(matches!(state.overlay(), OverlayState::Activity(_)));
  state.apply_overlay_key(
    key(
      KeyCode::Char('o'),
      KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ),
    20,
  );
  assert!(matches!(state.overlay(), OverlayState::Activity(_)));
  assert_eq!(command_catalog().len(), 10);
  assert!(slash_command("/activity").command_id().is_none());
}

#[test]
fn page_navigation_uses_the_actual_responsive_list_viewport() {
  let (mut compact, compact_targets) = state_with_steps(20);
  compact.open_activity();
  compact.apply_overlay_key_with_viewport(key(KeyCode::PageUp, KeyModifiers::NONE), 52, 12);
  assert_activity(&compact, compact_targets[18], ActivityFocus::List, false);

  let (mut wide, wide_targets) = state_with_steps(20);
  wide.open_activity();
  wide.apply_overlay_key_with_viewport(key(KeyCode::PageUp, KeyModifiers::NONE), 90, 20);
  assert_activity(&wide, wide_targets[4], ActivityFocus::List, false);
}

#[test]
fn detail_scroll_is_clamped_to_the_latest_render_metrics() {
  let (mut state, _) = state_with_steps(1);
  state.open_activity();
  state.apply_overlay_key(key(KeyCode::Enter, KeyModifiers::NONE), 20);
  state.apply_overlay_key(key(KeyCode::End, KeyModifiers::NONE), 20);

  state.clamp_activity_detail_scroll(7);

  let OverlayState::Activity(inspector) = state.overlay() else {
    panic!("activity inspector closed")
  };
  assert_eq!(inspector.detail_scroll, 7);
}

#[test]
fn model_setup_arrival_closes_the_inspector_without_restoring_it() {
  let (mut state, _) = state_with_steps(1);
  state.open_activity();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(ModelSetupView {
      source: ModelSetupSource::ConfiguredPath,
      phase: ModelSetupPhase::Required,
      managed_model: None,
      operation_id: None,
      downloaded_bytes: 0,
      total_bytes: None,
      failure: None,
      allowed_actions: vec![],
    }),
    ..RuntimeSnapshot::default()
  });
  assert_eq!(state.overlay(), &OverlayState::Closed);

  state.sync_snapshot(&snapshot_with_steps(1));
  assert_eq!(state.overlay(), &OverlayState::Closed);
}

#[test]
fn language_toggle_preserves_inspector_selection_and_scroll() {
  let (state, _) = state_with_steps(2);
  let (_, snapshots) = watch::channel(RuntimeSnapshot::default());
  let mut runtime = Runtime {
    state,
    client: None,
    snapshots,
    remote_state: None,
    approval: None,
    approval_scroll: 0,
    approval_max_scroll: 0,
    conversation_scroll: 0,
    conversation_max_scroll: 0,
    locale: Locale::EnUs,
  };
  runtime.state.open_activity();
  runtime
    .state
    .apply_overlay_key(key(KeyCode::Enter, KeyModifiers::NONE), 20);
  runtime
    .state
    .apply_overlay_key(key(KeyCode::Down, KeyModifiers::NONE), 20);
  let before = runtime.state.overlay().clone();

  assert!(runtime.apply_language_command(LanguageCommand::Toggle));

  assert_eq!(runtime.locale(), Locale::ZhCn);
  assert_eq!(runtime.state.overlay(), &before);
}

fn assert_activity(
  state: &TuiState,
  target: ActivityTarget,
  focus: ActivityFocus,
  follow_latest: bool,
) {
  let OverlayState::Activity(inspector) = state.overlay() else {
    panic!("activity inspector not open")
  };
  assert_eq!(inspector.selected, Some(target));
  assert_eq!(inspector.focus, focus);
  assert_eq!(inspector.follow_latest, follow_latest);
}

fn state_with_steps(count: usize) -> (TuiState, Vec<ActivityTarget>) {
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot_with_steps(count));
  let targets = state.activities().iter().map(|item| item.target).collect();
  (state, targets)
}

fn snapshot_with_steps(count: usize) -> RuntimeSnapshot {
  let turn_id = TurnId::new();
  RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    turns: vec![TurnView {
      turn_id,
      user_text: "inspect".into(),
      steering: vec![],
      assistant_text: String::new(),
      status: TurnStatus::Running,
      activities: (0..count)
        .map(|index| ActivityView {
          step_id: StepId::new(),
          phase: ActivityPhase::Streaming,
          route: None,
          context: None,
          assistant_text: format!("step {index}"),
          usage: None,
          tools: vec![],
          outcome: None,
        })
        .collect(),
    }],
    ..RuntimeSnapshot::default()
  }
}

fn connected(server: &str, snapshot: RuntimeSnapshot) -> RemoteState {
  connected_revision(server, 1, snapshot)
}

fn connected_revision(server: &str, revision: u64, snapshot: RuntimeSnapshot) -> RemoteState {
  RemoteState {
    connection: ConnectionState::Connected {
      server_instance_id: server.into(),
    },
    snapshot: Some(VersionedRemoteSnapshot {
      server_instance_id: server.into(),
      revision,
      snapshot,
    }),
  }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
  KeyEvent::new(code, modifiers)
}
