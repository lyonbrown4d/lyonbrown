//! AgentEvent：事件驱动 Kernel 的契约（§29）。
//!
//! 事件日志是 resume / replay / audit / redaction 扫描 / e2e 断言的唯一事实源。
//! serde 用 `tag = "type"`，tag 值即变体名（与 [`AgentEvent::kind`] 一致），便于回放与审计。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{ObservationId, SessionId, StepId, ToolCallId, TurnId};
use crate::model::{StopReason, TokenUsage};
use crate::tool::{ToolInvocation, ToolResult};

/// 一个 step 的产出（喂给 quality gate 与下一 step）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepOutcome {
  pub stop_reason: StopReason,
  pub text: String,
  pub tool_calls: Vec<ToolCallId>,
  pub escalated: bool,
}

/// §29 全量事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
  SessionStarted {
    session_id: SessionId,
    ts: DateTime<Utc>,
  },
  UserMessageReceived {
    session_id: SessionId,
    turn_id: TurnId,
    text: String,
    ts: DateTime<Utc>,
  },
  TurnStarted {
    session_id: SessionId,
    turn_id: TurnId,
    ts: DateTime<Utc>,
  },
  TurnSteered {
    session_id: SessionId,
    turn_id: TurnId,
    text: String,
    ts: DateTime<Utc>,
  },
  StepStarted {
    session_id: SessionId,
    turn_id: TurnId,
    step_id: StepId,
    ts: DateTime<Utc>,
  },
  ContextPrepared {
    step_id: StepId,
    system_tokens: u32,
    context_tokens: u32,
    dropped: u32,
    ts: DateTime<Utc>,
  },
  ModelSelected {
    step_id: StepId,
    provider: String,
    model: String,
    reason: String,
    ts: DateTime<Utc>,
  },
  ModelFallback {
    step_id: StepId,
    from_provider: String,
    from_model: String,
    to_provider: String,
    to_model: String,
    reason: String,
    ts: DateTime<Utc>,
  },
  ModelStarted {
    step_id: StepId,
    ts: DateTime<Utc>,
  },
  ModelDelta {
    step_id: StepId,
    delta: String,
  },
  ModelCompleted {
    step_id: StepId,
    usage: TokenUsage,
    ts: DateTime<Utc>,
  },
  ToolRequested {
    step_id: StepId,
    invocation: ToolInvocation,
    ts: DateTime<Utc>,
  },
  ToolApproved {
    step_id: StepId,
    tool_call_id: ToolCallId,
    by: String,
    ts: DateTime<Utc>,
  },
  ToolStarted {
    step_id: StepId,
    tool_call_id: ToolCallId,
    ts: DateTime<Utc>,
  },
  ToolCompleted {
    step_id: StepId,
    result: ToolResult,
    ts: DateTime<Utc>,
  },
  ObservationCreated {
    step_id: StepId,
    observation_id: ObservationId,
    ts: DateTime<Utc>,
  },
  StepCompleted {
    step_id: StepId,
    outcome: StepOutcome,
    ts: DateTime<Utc>,
  },
  TurnCompleted {
    turn_id: TurnId,
    answer: String,
    ts: DateTime<Utc>,
  },
  TurnCancelled {
    session_id: SessionId,
    turn_id: TurnId,
    ts: DateTime<Utc>,
  },
  SessionClosed {
    session_id: SessionId,
    ts: DateTime<Utc>,
  },
  Error {
    session_id: SessionId,
    message: String,
    ts: DateTime<Utc>,
  },
}

impl AgentEvent {
  /// 变体名（与 serde `type` tag 一致）。
  pub fn kind(&self) -> &'static str {
    match self {
      Self::SessionStarted { .. } => "SessionStarted",
      Self::UserMessageReceived { .. } => "UserMessageReceived",
      Self::TurnStarted { .. } => "TurnStarted",
      Self::TurnSteered { .. } => "TurnSteered",
      Self::StepStarted { .. } => "StepStarted",
      Self::ContextPrepared { .. } => "ContextPrepared",
      Self::ModelSelected { .. } => "ModelSelected",
      Self::ModelFallback { .. } => "ModelFallback",
      Self::ModelStarted { .. } => "ModelStarted",
      Self::ModelDelta { .. } => "ModelDelta",
      Self::ModelCompleted { .. } => "ModelCompleted",
      Self::ToolRequested { .. } => "ToolRequested",
      Self::ToolApproved { .. } => "ToolApproved",
      Self::ToolStarted { .. } => "ToolStarted",
      Self::ToolCompleted { .. } => "ToolCompleted",
      Self::ObservationCreated { .. } => "ObservationCreated",
      Self::StepCompleted { .. } => "StepCompleted",
      Self::TurnCompleted { .. } => "TurnCompleted",
      Self::TurnCancelled { .. } => "TurnCancelled",
      Self::SessionClosed { .. } => "SessionClosed",
      Self::Error { .. } => "Error",
    }
  }

  /// 若为 `ToolCompleted`，返回其结果。
  pub fn tool_result(&self) -> Option<&ToolResult> {
    match self {
      Self::ToolCompleted { result, .. } => Some(result),
      _ => None,
    }
  }

  /// 若为 `TurnCompleted`，返回最终回答。
  pub fn turn_answer(&self) -> Option<&str> {
    match self {
      Self::TurnCompleted { answer, .. } => Some(answer),
      _ => None,
    }
  }
}
