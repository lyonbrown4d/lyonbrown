//! Daemon-authored presentation events, independent of provider and runtime SDKs.
//!
//! Clients render these facts without deriving execution phases from persisted
//! runtime records. Only typed events and current daemon statuses are decoded.

mod decode;

pub use decode::{DecodedEvent, decode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wire kind carrying a complete, internally tagged [`AgentEvent`] payload.
pub const AGENT_EVENT_KIND: &str = "agent_event";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
  PhaseChanged {
    phase: ExecutionPhase,
    model: Option<ModelIdentity>,
  },
  TextDelta {
    text: String,
  },
  ThinkingDelta {
    text: String,
  },
  ToolStarted {
    call_id: String,
    name: String,
    arguments: Value,
  },
  ToolFinished {
    call_id: String,
    name: String,
    text: String,
    is_error: bool,
  },
  RunFinished {
    run_id: String,
    outcome: ExecutionOutcome,
  },
  Compaction {
    active: bool,
  },
  UsageUpdated {
    entry_id: String,
    usage: TokenUsage,
  },
  QueueUpdated {
    entry_id: String,
    kind: QueueKind,
    status: QueueStatus,
    prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
  },
  RunStateChanged {
    status: crate::RunStatus,
    error: String,
  },
}

impl AgentEvent {
  pub fn kind(&self) -> &'static str {
    AGENT_EVENT_KIND
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPhase {
  Preparing,
  ConnectingModel,
  Generating,
  Thinking,
  Responding,
  PreparingTool,
  ExecutingTool,
  AwaitingApproval,
  Cancelling,
  Compacting,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentity {
  pub provider: String,
  pub model: String,
  pub context_window: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
  Completed,
  Failed,
  Cancelled,
  Uncertain,
  Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueKind {
  Steer,
  FollowUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueStatus {
  Pending,
  CancelRequested,
  Consumed,
  Cancelled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
  pub input: u64,
  pub output: u64,
  pub cache_read: u64,
  pub cache_write: u64,
  pub total: u64,
}
