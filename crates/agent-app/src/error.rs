use agent_core::{AgentError, ToolCallId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedCommand {
  ForceStop,
  RespondApproval,
  SelectSessionModel,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCommandError {
  #[error("the runtime actor is no longer available")]
  RuntimeStopped,
  #[error("a turn is already active")]
  TurnAlreadyActive,
  #[error("there is no active turn")]
  NoActiveTurn,
  #[error("command text must not be empty")]
  EmptyText,
  #[error("kernel rejected steering: {0}")]
  Steering(String),
  #[error("there is no pending approval request")]
  NoPendingApproval,
  #[error("approval request id mismatch: expected {expected:?}, received {received:?}")]
  ApprovalRequestMismatch {
    expected: ToolCallId,
    received: ToolCallId,
  },
  #[error("command is not supported by the current kernel: {0:?}")]
  Unsupported(UnsupportedCommand),
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeActorError {
  #[error("kernel event stream closed unexpectedly")]
  EventStreamClosed,
  #[error("kernel event stream lagged by {0} events")]
  EventStreamLagged(u64),
  #[error("turn task failed: {0}")]
  TurnTask(String),
  #[error("runtime task failed: {0}")]
  RuntimeTask(String),
  #[error("kernel turn failed: {0}")]
  Kernel(#[source] AgentError),
}
