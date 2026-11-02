//! Desktop projection of daemon-authored events. Unknown payloads are discarded.

use agent_core::{
  RunStatus,
  events::{self, AgentEvent, ExecutionPhase, ModelIdentity, TokenUsage},
};
use agent_protocol::v1::Event;

pub(crate) const APPROVAL_REQUIRED: &str = "approval_required";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalOutcome {
  Completed,
  Failed,
  Cancelled,
  Uncertain,
}

impl TerminalOutcome {
  pub(crate) fn from_kind(kind: &str) -> Option<Self> {
    match kind {
      "run_completed" => Some(Self::Completed),
      "run_failed" => Some(Self::Failed),
      "run_cancelled" => Some(Self::Cancelled),
      "run_uncertain" => Some(Self::Uncertain),
      _ => None,
    }
  }
}

/// Watch termination remains tied to durable run lifecycle events. A harness
/// `run_finished` fact does not establish the durable run's terminal state.
pub(crate) fn is_terminal(kind: &str) -> bool {
  TerminalOutcome::from_kind(kind).is_some()
}

pub(crate) fn is_approval_required(kind: &str) -> bool {
  kind == APPROVAL_REQUIRED
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RunEvent {
  TextDelta {
    text: String,
  },
  ToolStarted {
    name: String,
  },
  ToolFinished {
    name: String,
  },
  PhaseChanged {
    phase: ExecutionPhase,
    model: Option<ModelIdentity>,
  },
  UsageUpdated {
    usage: TokenUsage,
  },
  StateChanged {
    status: RunStatus,
  },
  ApprovalRequired,
  /// Known facts without a separate transcript projection.
  Activity,
  Other,
}

/// Only reviewed presentation fields cross into the view model.
#[derive(Clone, Debug)]
pub(crate) struct DecodedEvent {
  pub(crate) kind: String,
  pub(crate) event: RunEvent,
}

impl DecodedEvent {
  pub(crate) fn status(&self) -> Option<&str> {
    match self.event {
      RunEvent::StateChanged { status } => Some(status.as_str()),
      RunEvent::ApprovalRequired => Some("awaiting_approval"),
      _ => None,
    }
  }
}

pub(crate) fn decode(event: &Event) -> DecodedEvent {
  let (kind, body) = match events::decode(&event.kind, &event.payload_json) {
    Ok(events::DecodedEvent::Known(known)) => {
      let body = match known {
        AgentEvent::TextDelta { text } => RunEvent::TextDelta { text },
        AgentEvent::ToolStarted { name, .. } => RunEvent::ToolStarted { name },
        AgentEvent::ToolFinished { name, .. } => RunEvent::ToolFinished { name },
        AgentEvent::PhaseChanged { phase, model } => RunEvent::PhaseChanged { phase, model },
        AgentEvent::UsageUpdated { usage, .. } => RunEvent::UsageUpdated { usage },
        AgentEvent::RunStateChanged { status, .. } => RunEvent::StateChanged { status },
        _ => RunEvent::Activity,
      };
      (event.kind.clone(), body)
    }
    Ok(events::DecodedEvent::Unknown { kind }) => {
      let body = if event.kind == APPROVAL_REQUIRED {
        RunEvent::ApprovalRequired
      } else {
        RunEvent::Other
      };
      (kind, body)
    }
    Err(_) => (event.kind.clone(), RunEvent::Other),
  };
  DecodedEvent { kind, event: body }
}

/// Labels mirror explicit daemon facts; no phase is inferred from deltas or
/// persistence/audit records.
pub(crate) fn phase_label(phase: ExecutionPhase, model: Option<&ModelIdentity>) -> String {
  let phase = match phase {
    ExecutionPhase::Preparing => "Preparing",
    ExecutionPhase::ConnectingModel => "Connecting model",
    ExecutionPhase::Generating => "Generating",
    ExecutionPhase::Thinking => "Thinking",
    ExecutionPhase::Responding => "Responding",
    ExecutionPhase::PreparingTool => "Preparing tool",
    ExecutionPhase::ExecutingTool => "Executing tool",
    ExecutionPhase::AwaitingApproval => "Awaiting approval",
    ExecutionPhase::Cancelling => "Cancelling",
    ExecutionPhase::Compacting => "Compacting",
  };
  model.map_or_else(
    || phase.into(),
    |model| format!("{phase} · {} / {}", model.provider, model.model),
  )
}
