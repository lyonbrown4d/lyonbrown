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
      KernelState::Failed {
        error: "existing".into(),
      },
      AgentEvent::UserMessageReceived {
        session_id,
        turn_id,
        text: "retry".into(),
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
      AgentEvent::ModelFallback {
        step_id,
        from_provider: "local".into(),
        from_model: "local-model".into(),
        to_provider: "remote".into(),
        to_model: "remote-model".into(),
        reason: "timeout".into(),
        ts: now,
      },
      KernelState::SelectingModel,
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

#[path = "state_machine/extended.rs"]
mod extended;
