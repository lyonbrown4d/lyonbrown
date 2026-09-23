use agent_cli::tui::TuiState;
use agent_core::{AgentEvent, StepId};
use chrono::Utc;

#[test]
fn fallback_event_updates_visible_status() {
  let mut state = TuiState::default();
  state.reduce(AgentEvent::ModelFallback {
    step_id: StepId::new(),
    from_provider: "ollama".into(),
    from_model: "local".into(),
    to_provider: "anthropic".into(),
    to_model: "remote".into(),
    reason: "timeout".into(),
    ts: Utc::now(),
  });

  assert!(state.status().contains("anthropic/remote"));
}
