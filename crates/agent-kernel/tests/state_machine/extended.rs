use super::*;

#[test]
fn documented_transition_table_is_accepted() {
  for (from, event, expected) in transition_events() {
    let actual = from.transition(&event).unwrap();
    assert_eq!(actual, expected, "event {} from {from:?}", event.kind());
  }
}

#[test]
fn illegal_transition_is_rejected_without_changing_state() {
  let event = AgentEvent::ModelStarted {
    step_id: StepId::new(),
    ts: Utc::now(),
  };

  let error = KernelState::Idle.transition(&event).unwrap_err();

  assert!(error.to_string().contains("Idle"));
  assert!(error.to_string().contains("ModelStarted"));
}

#[test]
fn every_state_event_pair_is_either_documented_or_rejected() {
  let cases = transition_events();
  let states = vec![
    KernelState::Idle,
    KernelState::PreparingContext,
    KernelState::SelectingModel,
    KernelState::ModelRunning,
    KernelState::ToolRequired,
    KernelState::ToolRunning,
    KernelState::Observation,
    KernelState::Completed,
    KernelState::Failed {
      error: "existing".into(),
    },
    KernelState::Cancelled,
  ];
  let events: Vec<_> = cases.iter().map(|(_, event, _)| event.clone()).collect();

  for state in &states {
    for event in &events {
      let expected = expected_transition(state, event, &cases);
      let actual = state.transition(event);
      match expected {
        Some(expected) => assert_eq!(
          actual.unwrap(),
          expected,
          "event {} from {state:?}",
          event.kind()
        ),
        None => assert!(
          actual.is_err(),
          "unexpectedly accepted event {} from {state:?}",
          event.kind()
        ),
      }
    }
  }
}

fn expected_transition(
  state: &KernelState,
  event: &AgentEvent,
  cases: &[(KernelState, AgentEvent, KernelState)],
) -> Option<KernelState> {
  if let AgentEvent::Error { message, .. } = event {
    return Some(KernelState::Failed {
      error: message.clone(),
    });
  }
  if matches!(state, KernelState::Cancelled)
    && matches!(event, AgentEvent::UserMessageReceived { .. })
  {
    return Some(KernelState::Idle);
  }
  if matches!(event, AgentEvent::TurnCancelled { .. })
    && matches!(
      state,
      KernelState::PreparingContext
        | KernelState::SelectingModel
        | KernelState::ModelRunning
        | KernelState::ToolRequired
        | KernelState::ToolRunning
        | KernelState::Observation
    )
  {
    return Some(KernelState::Cancelled);
  }

  cases
    .iter()
    .find(|(from, candidate, _)| from == state && event_shape(candidate) == event_shape(event))
    .map(|(_, _, expected)| expected.clone())
}

fn event_shape(event: &AgentEvent) -> String {
  match event {
    AgentEvent::StepCompleted { outcome, .. } => format!(
      "StepCompleted:{:?}:{}",
      outcome.stop_reason, outcome.escalated
    ),
    _ => event.kind().to_owned(),
  }
}
