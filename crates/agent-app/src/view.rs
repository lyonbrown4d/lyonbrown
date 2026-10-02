use std::time::Instant;

use agent_core::{StepId, StepOutcome, TokenUsage, ToolCallId, ToolPreview, ToolResult, TurnId};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RuntimePhase {
  #[default]
  Initializing,
  ModelSetupRequired,
  Ready,
  Preparing,
  Streaming,
  AwaitingApproval,
  RunningTool,
  Cancelling,
  Completed,
  FailedRecoverable,
  FailedTerminal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ModelSelection {
  #[default]
  Auto,
  Pinned {
    provider: String,
    model: String,
  },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
  pub provider: String,
  pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFallbackView {
  pub from: ModelRoute,
  pub to: ModelRoute,
  pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSummary {
  pub selected: ModelRoute,
  pub reason: String,
  pub fallbacks: Vec<ModelFallbackView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSetupSource {
  ManagedPreset,
  ConfiguredPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSetupPhase {
  Required,
  WaitingForLock,
  Checking,
  Downloading,
  Verifying,
  Installing,
  Activating,
  Ready,
  Cancelled,
  Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSetupAction {
  Download,
  Cancel,
  Retry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedModelView {
  pub id: String,
  pub expected_download_bytes: u64,
  pub max_bytes: u64,
  pub min_available_memory_bytes: u64,
  pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSetupFailureView {
  pub code: String,
  pub message: String,
  pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSetupView {
  pub source: ModelSetupSource,
  pub phase: ModelSetupPhase,
  pub managed_model: Option<ManagedModelView>,
  pub operation_id: Option<String>,
  pub downloaded_bytes: u64,
  pub total_bytes: Option<u64>,
  pub failure: Option<ModelSetupFailureView>,
  pub allowed_actions: Vec<ModelSetupAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
  Pending,
  Running,
  Completed,
  Cancelled,
  Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityPhase {
  Preparing,
  Streaming,
  AwaitingApproval,
  RunningTool,
  Superseded,
  Completed,
  Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSummary {
  pub system_tokens: u32,
  pub context_tokens: u32,
  pub dropped: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolActivityState {
  Requested,
  Approved,
  Running,
  Completed,
  Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolActivityView {
  pub call_id: ToolCallId,
  pub name: String,
  pub arguments: Value,
  pub state: ToolActivityState,
  pub approved_by: Option<String>,
  pub result: Option<ToolResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityView {
  pub step_id: StepId,
  pub phase: ActivityPhase,
  pub route: Option<RouteSummary>,
  pub context: Option<ContextSummary>,
  pub assistant_text: String,
  pub usage: Option<TokenUsage>,
  pub tools: Vec<ToolActivityView>,
  pub outcome: Option<StepOutcome>,
}

impl ActivityView {
  pub(crate) fn new(step_id: StepId) -> Self {
    Self {
      step_id,
      phase: ActivityPhase::Preparing,
      route: None,
      context: None,
      assistant_text: String::new(),
      usage: None,
      tools: Vec::new(),
      outcome: None,
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnView {
  pub turn_id: TurnId,
  pub user_text: String,
  /// Later user guidance injected at safe boundaries of this active turn.
  pub steering: Vec<String>,
  pub assistant_text: String,
  pub status: TurnStatus,
  pub activities: Vec<ActivityView>,
}

impl TurnView {
  pub(crate) fn new(turn_id: TurnId, user_text: String) -> Self {
    Self {
      turn_id,
      user_text,
      steering: Vec::new(),
      assistant_text: String::new(),
      status: TurnStatus::Pending,
      activities: Vec::new(),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalState {
  Pending,
  Approved,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalView {
  pub request_id: ToolCallId,
  pub step_id: StepId,
  pub tool_name: String,
  pub arguments: Value,
  pub preview: Option<ToolPreview>,
  pub deadline: Option<Instant>,
  pub state: ApprovalState,
  pub approved_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeSnapshot {
  pub phase: RuntimePhase,
  pub session_id: Option<agent_core::SessionId>,
  pub turns: Vec<TurnView>,
  pub approval: Option<ApprovalView>,
  pub queued_follow_ups: Vec<String>,
  pub model_selection: ModelSelection,
  pub route: Option<RouteSummary>,
  pub last_error: Option<String>,
  pub model_setup: Option<ModelSetupView>,
}

impl Default for RuntimeSnapshot {
  fn default() -> Self {
    Self {
      phase: RuntimePhase::Initializing,
      session_id: None,
      turns: Vec::new(),
      approval: None,
      queued_follow_ups: Vec::new(),
      model_selection: ModelSelection::Auto,
      route: None,
      last_error: None,
      model_setup: None,
    }
  }
}
