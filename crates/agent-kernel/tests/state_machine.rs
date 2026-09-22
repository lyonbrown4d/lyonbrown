use agent_core::{
    AgentEvent, ObservationId, SessionId, StepId, StepOutcome, StopReason, TokenUsage, ToolCallId,
    ToolInvocation, ToolResult, TurnId,
};
use agent_kernel::KernelState;
use chrono::Utc;
use serde_json::json;

fn transition_events() -> Vec<(KernelState, AgentEvent, KernelState)> {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let step_id = StepId::new();
    let call_id = ToolCallId::new();
    let now = Utc::now();
    let invocation = ToolInvocation {
        id: call_id,
        name: "echo".into(),
        arguments: json!({"text": "hello"}),
    };

    vec![
        (
            KernelState::Idle,
            AgentEvent::SessionStarted {
                session_id,
                ts: now,
            },
            KernelState::Idle,
        ),
        (
            KernelState::Idle,
            AgentEvent::UserMessageReceived {
                session_id,
                turn_id,
                text: "hello".into(),
                ts: now,
            },
            KernelState::Idle,
        ),
        (
            KernelState::Idle,
            AgentEvent::TurnStarted {
                session_id,
                turn_id,
                ts: now,
            },
            KernelState::PreparingContext,
        ),
        (
            KernelState::PreparingContext,
            AgentEvent::StepStarted {
                session_id,
                turn_id,
                step_id,
                ts: now,
            },
            KernelState::PreparingContext,
        ),
        (
            KernelState::PreparingContext,
            AgentEvent::ContextPrepared {
                step_id,
                system_tokens: 0,
                context_tokens: 1,
                dropped: 0,
                ts: now,
            },
            KernelState::SelectingModel,
        ),
        (
            KernelState::SelectingModel,
            AgentEvent::ModelSelected {
                step_id,
                provider: "mock".into(),
                model: "mock-model".into(),
                reason: "first available".into(),
                ts: now,
            },
            KernelState::ModelRunning,
        ),
        (
            KernelState::ModelRunning,
            AgentEvent::ModelStarted { step_id, ts: now },
            KernelState::ModelRunning,
        ),
        (
            KernelState::ModelRunning,
            AgentEvent::ModelDelta {
                step_id,
                delta: "hello".into(),
            },
            KernelState::ModelRunning,
        ),
        (
            KernelState::ModelRunning,
            AgentEvent::ModelCompleted {
                step_id,
                usage: TokenUsage::default(),
                ts: now,
            },
            KernelState::ModelRunning,
        ),
        (
            KernelState::ModelRunning,
            AgentEvent::ToolRequested {
                step_id,
                invocation: invocation.clone(),
                ts: now,
            },
            KernelState::ToolRequired,
        ),
        (
            KernelState::Observation,
            AgentEvent::ToolRequested {
                step_id,
                invocation: invocation.clone(),
                ts: now,
            },
            KernelState::ToolRequired,
        ),
        (
            KernelState::ToolRequired,
            AgentEvent::ToolApproved {
                step_id,
                tool_call_id: call_id,
                by: "policy".into(),
                ts: now,
            },
            KernelState::ToolRunning,
        ),
        (
            KernelState::ToolRunning,
            AgentEvent::ToolStarted {
                step_id,
                tool_call_id: call_id,
                ts: now,
            },
            KernelState::ToolRunning,
        ),
        (
            KernelState::ToolRunning,
            AgentEvent::ToolCompleted {
                step_id,
                result: ToolResult {
                    invocation_id: call_id,
                    ok: true,
                    output: "hello".into(),
                    truncated: false,
                    duration_ms: 1,
                    error: None,
                },
                ts: now,
            },
            KernelState::Observation,
        ),
        (
            KernelState::ToolRequired,
            AgentEvent::ToolCompleted {
                step_id,
                result: ToolResult {
                    invocation_id: call_id,
                    ok: false,
                    output: String::new(),
                    truncated: false,
                    duration_ms: 0,
                    error: Some("tool unavailable".into()),
                },
                ts: now,
            },
            KernelState::Observation,
        ),
        (
            KernelState::Observation,
            AgentEvent::ObservationCreated {
                step_id,
                observation_id: ObservationId::new(),
                ts: now,
            },
            KernelState::Observation,
        ),
        (
            KernelState::Observation,
            AgentEvent::StepCompleted {
                step_id,
                outcome: StepOutcome {
                    stop_reason: StopReason::ToolUse,
                    text: String::new(),
                    tool_calls: vec![call_id],
                    escalated: false,
                },
                ts: now,
            },
            KernelState::PreparingContext,
        ),
        (
            KernelState::ModelRunning,
            AgentEvent::StepCompleted {
                step_id,
                outcome: StepOutcome {
                    stop_reason: StopReason::EndTurn,
                    text: "done".into(),
                    tool_calls: vec![],
                    escalated: false,
                },
                ts: now,
            },
            KernelState::Completed,
        ),
        (
            KernelState::ModelRunning,
            AgentEvent::StepCompleted {
                step_id,
                outcome: StepOutcome {
                    stop_reason: StopReason::EndTurn,
                    text: String::new(),
                    tool_calls: vec![],
                    escalated: true,
                },
                ts: now,
            },
            KernelState::PreparingContext,
        ),
        (
            KernelState::Completed,
            AgentEvent::TurnCompleted {
                turn_id,
                answer: "done".into(),
                ts: now,
            },
            KernelState::Idle,
        ),
        (
            KernelState::ModelRunning,
            AgentEvent::TurnCancelled {
                session_id,
                turn_id,
                ts: now,
            },
            KernelState::Cancelled,
        ),
        (
            KernelState::Idle,
            AgentEvent::SessionClosed {
                session_id,
                ts: now,
            },
            KernelState::Idle,
        ),
        (
            KernelState::SelectingModel,
            AgentEvent::Error {
                session_id,
                message: "boom".into(),
                ts: now,
            },
            KernelState::Failed {
                error: "boom".into(),
            },
        ),
    ]
}

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
