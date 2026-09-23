use agent_gui::state::{CockpitState, RuntimeDetailsPlacement};

#[test]
fn runtime_details_start_collapsed() {
  let state = CockpitState::default();

  assert!(!state.runtime_details_expanded());
  assert_eq!(state.runtime_details_control_label(), "Show details");
}

#[test]
fn runtime_details_control_toggles_preview_visibility() {
  let mut state = CockpitState::default();

  state.toggle_runtime_details();
  assert!(state.runtime_details_expanded());
  assert_eq!(state.runtime_details_control_label(), "Hide details");

  state.toggle_runtime_details();
  assert!(!state.runtime_details_expanded());
  assert_eq!(state.runtime_details_control_label(), "Show details");
}

#[test]
fn expanded_details_render_in_the_visible_region_for_each_layout() {
  assert_eq!(
    RuntimeDetailsPlacement::for_layout(false, false),
    RuntimeDetailsPlacement::Hidden
  );
  assert_eq!(
    RuntimeDetailsPlacement::for_layout(true, true),
    RuntimeDetailsPlacement::Conversation
  );
  assert_eq!(
    RuntimeDetailsPlacement::for_layout(false, true),
    RuntimeDetailsPlacement::RuntimeRail
  );
}
