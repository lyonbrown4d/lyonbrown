use agent_core::ToolCallId;

use crate::ModelSelection;

/// A user decision for a pending risky tool invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
  AllowOnce,
  AllowForSession,
  DenyOnce,
  DenyRemaining,
}

/// Commands accepted by the application runtime actor.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentCommand {
  /// Start a new turn with the supplied user message.
  StartTurn { text: String },
  /// Guide the active turn at its next safe execution boundary.
  SteerCurrentTurn { text: String },
  /// Start a new turn with this text after the active turn completes.
  QueueFollowUp { text: String },
  /// Request cooperative cancellation of the active turn.
  CancelTurn,
  /// Stop the active task without waiting for cooperative cancellation.
  ForceStop,
  /// Resolve a pending tool approval request.
  RespondApproval {
    request_id: ToolCallId,
    decision: ApprovalDecision,
  },
  /// Change the session model policy from the next safe step onward.
  SelectSessionModel { selection: ModelSelection },
}
