use agent_app::{
  ActivityPhase, ApprovalState, Projector, RuntimePhase, ToolActivityState, TurnStatus,
};
use agent_core::{
  AgentEvent, SessionId, StepId, StepOutcome, StopReason, TokenUsage, ToolCallId, ToolInvocation,
  ToolResult, TurnId,
};
use chrono::Utc;

struct Trace {
  session: SessionId,
  turn: TurnId,
  step: StepId,
}

impl Trace {
  fn new() -> Self {
    Self {
      session: SessionId::new(),
      turn: TurnId::new(),
      step: StepId::new(),
    }
  }

  fn begin(&self) -> Vec<AgentEvent> {
    vec![
      AgentEvent::SessionStarted {
        session_id: self.session,
        ts: Utc::now(),
      },
      AgentEvent::UserMessageReceived {
        session_id: self.session,
        turn_id: self.turn,
        text: "hello".into(),
        ts: Utc::now(),
      },
      AgentEvent::TurnStarted {
        session_id: self.session,
        turn_id: self.turn,
        ts: Utc::now(),
      },
      AgentEvent::StepStarted {
        session_id: self.session,
        turn_id: self.turn,
        step_id: self.step,
        ts: Utc::now(),
      },
    ]
  }
}

#[test]
fn projects_running_streaming_and_completed_without_duplicate_answer() {
  let trace = Trace::new();
  let mut events = trace.begin();
  events.extend([
    AgentEvent::ModelSelected {
      step_id: trace.step,
      provider: "local".into(),
      model: "worker".into(),
      reason: "local first".into(),
      ts: Utc::now(),
    },
    AgentEvent::ModelStarted {
      step_id: trace.step,
      ts: Utc::now(),
    },
    AgentEvent::ModelDelta {
      step_id: trace.step,
      delta: "hel".into(),
    },
    AgentEvent::ModelDelta {
      step_id: trace.step,
      delta: "lo".into(),
    },
  ]);
  let mut projector = Projector::default();
  projector.extend(events);

  assert_eq!(projector.snapshot().phase, RuntimePhase::Streaming);
  let turn = &projector.snapshot().turns[0];
  assert_eq!(turn.assistant_text, "hello");
  assert_eq!(turn.activities[0].phase, ActivityPhase::Streaming);
  assert_eq!(turn.activities[0].assistant_text, "hello");
  assert_eq!(
    turn.activities[0].route.as_ref().unwrap().selected.model,
    "worker"
  );

  projector.apply(AgentEvent::ModelCompleted {
    step_id: trace.step,
    usage: TokenUsage {
      input: 2,
      output: 1,
      cache_read: 0,
    },
    ts: Utc::now(),
  });
  projector.apply(AgentEvent::StepCompleted {
    step_id: trace.step,
    outcome: StepOutcome {
      stop_reason: StopReason::EndTurn,
      text: "hello".into(),
      tool_calls: vec![],
      escalated: false,
    },
    ts: Utc::now(),
  });
  projector.apply(AgentEvent::TurnCompleted {
    turn_id: trace.turn,
    answer: "hello".into(),
    ts: Utc::now(),
  });

  let snapshot = projector.snapshot();
  assert_eq!(snapshot.phase, RuntimePhase::Completed);
  assert_eq!(snapshot.turns[0].status, TurnStatus::Completed);
  assert_eq!(snapshot.turns[0].assistant_text, "hello");
  assert_eq!(
    snapshot.turns[0].activities[0].phase,
    ActivityPhase::Completed
  );
  assert_eq!(snapshot.turns[0].activities[0].usage.unwrap().output, 1);
}

#[test]
fn groups_tool_activity_and_approval_with_the_step() {
  let trace = Trace::new();
  let call_id = ToolCallId::new();
  let mut projector = Projector::default();
  projector.extend(trace.begin());
  projector.apply(AgentEvent::ToolRequested {
    step_id: trace.step,
    invocation: ToolInvocation {
      id: call_id,
      name: "shell".into(),
      arguments: serde_json::json!({"command": "cargo test"}),
    },
    ts: Utc::now(),
  });

  assert_eq!(projector.snapshot().phase, RuntimePhase::AwaitingApproval);
  let approval = projector.snapshot().approval.as_ref().unwrap();
  assert_eq!(approval.request_id, call_id);
  assert_eq!(approval.state, ApprovalState::Pending);

  projector.apply(AgentEvent::ToolApproved {
    step_id: trace.step,
    tool_call_id: call_id,
    by: "user".into(),
    ts: Utc::now(),
  });
  projector.apply(AgentEvent::ToolStarted {
    step_id: trace.step,
    tool_call_id: call_id,
    ts: Utc::now(),
  });
  assert_eq!(projector.snapshot().phase, RuntimePhase::RunningTool);
  assert_eq!(
    projector.snapshot().turns[0].activities[0].tools[0].state,
    ToolActivityState::Running
  );

  projector.apply(AgentEvent::ToolCompleted {
    step_id: trace.step,
    result: ToolResult {
      invocation_id: call_id,
      ok: true,
      output: "ok".into(),
      truncated: false,
      duration_ms: 4,
      error: None,
    },
    ts: Utc::now(),
  });
  let activity = &projector.snapshot().turns[0].activities[0];
  assert_eq!(activity.tools[0].state, ToolActivityState::Completed);
  assert_eq!(activity.tools[0].result.as_ref().unwrap().output, "ok");
  assert!(projector.snapshot().approval.is_none());
}

#[test]
fn projects_fallback_error_and_cancel_while_preserving_transcript() {
  let trace = Trace::new();
  let mut projector = Projector::default();
  projector.extend(trace.begin());
  projector.apply(AgentEvent::ModelFallback {
    step_id: trace.step,
    from_provider: "local".into(),
    from_model: "small".into(),
    to_provider: "remote".into(),
    to_model: "frontier".into(),
    reason: "timeout".into(),
    ts: Utc::now(),
  });
  assert_eq!(
    projector.snapshot().route.as_ref().unwrap().fallbacks.len(),
    1
  );
  assert_eq!(
    projector
      .snapshot()
      .route
      .as_ref()
      .unwrap()
      .selected
      .provider,
    "remote"
  );

  projector.apply(AgentEvent::Error {
    session_id: trace.session,
    message: "provider unavailable".into(),
    ts: Utc::now(),
  });
  assert_eq!(projector.snapshot().phase, RuntimePhase::FailedRecoverable);
  assert_eq!(
    projector.snapshot().last_error.as_deref(),
    Some("provider unavailable")
  );
  assert_eq!(projector.snapshot().turns[0].user_text, "hello");

  projector.apply(AgentEvent::TurnCancelled {
    session_id: trace.session,
    turn_id: trace.turn,
    ts: Utc::now(),
  });
  assert_eq!(projector.snapshot().phase, RuntimePhase::Ready);
  assert_eq!(projector.snapshot().turns[0].status, TurnStatus::Cancelled);
  assert_eq!(projector.snapshot().turns[0].user_text, "hello");
}

#[test]
fn preserves_steering_as_part_of_the_running_turn() {
  let trace = Trace::new();
  let mut projector = Projector::default();
  projector.extend(trace.begin());

  projector.apply(AgentEvent::TurnSteered {
    session_id: trace.session,
    turn_id: trace.turn,
    text: "focus on the parser".into(),
    ts: Utc::now(),
  });

  let snapshot = projector.snapshot();
  assert_eq!(snapshot.phase, RuntimePhase::Preparing);
  assert_eq!(snapshot.turns[0].status, TurnStatus::Running);
  assert_eq!(snapshot.turns[0].steering, ["focus on the parser"]);
  assert_eq!(snapshot.turns[0].user_text, "hello");
  assert_eq!(
    snapshot.turns[0].activities[0].phase,
    ActivityPhase::Superseded
  );
}
