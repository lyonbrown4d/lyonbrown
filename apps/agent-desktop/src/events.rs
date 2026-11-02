//! Typed view of the run events agentd streams to the desktop client.
//!
//! agentd stores every run event as a `(kind, payload_json)` pair
//! (`agent_storage::EventStore::append_event_owned`); the client used to
//! compare those kind strings inline. This module keeps that mapping in one
//! place: run termination ([`is_terminal`]) and approval prompts
//! ([`is_approval_required`]) are shared by the watcher and the projection.
//!
//! Kinds produced by the daemon today:
//!
//! - runtime events (`agent_runtime::RuntimeEvent`, serialized with
//!   `#[serde(tag = "type", rename_all = "snake_case")]` after dropping the
//!   `type` field): `run_started`, `text_delta`, `thinking_delta`,
//!   `tool_started`, `tool_finished`, `run_finished`, `compaction`
//! - run status transitions (`agent_storage::runs`):
//!   `run_accepted`, `run_running`, `run_awaiting_approval`,
//!   `run_cancel_requested`, `run_completed`, `run_failed`, `run_cancelled`,
//!   `run_uncertain`
//! - approvals (`agent_storage::approvals`): `approval_required`,
//!   `approval_closed`, `approval_decided`, `tool_authorized`
//! - harness persistence (`agent_runtime::persistence`): `entry_appended`,
//!   `record_appended`
//! - daemon bookkeeping: `model_selected`, `execution_plan`,
//!   `model_route_decision`, `run_recovery_recorded`, `session_owner_changed`,
//!   `model_job_changed`
//!
//! Anything not recognized here decodes to [`RunEvent::Other`]; the client still
//! advances its cursor and refreshes the status line for those events.

use agent_protocol::v1::Event;
use serde_json::Value;

/// Kind emitted when a run is blocked on a human approval decision.
pub(crate) const APPROVAL_REQUIRED: &str = "approval_required";

/// How a run ended. Mirrors `agent_core::RunStatus::terminal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalOutcome {
  Completed,
  Failed,
  Cancelled,
  Uncertain,
}

impl TerminalOutcome {
  /// Classify a daemon event kind, or `None` when the kind is not terminal.
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

/// Whether `kind` ends the run and the watcher can stop streaming.
pub(crate) fn is_terminal(kind: &str) -> bool {
  TerminalOutcome::from_kind(kind).is_some()
}

/// Whether `kind` asks the client to fetch the run's pending approvals.
pub(crate) fn is_approval_required(kind: &str) -> bool {
  kind == APPROVAL_REQUIRED
}

/// The part of a daemon event the desktop client actually renders.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RunEvent {
  /// `text_delta`; `text` is `None` when the payload is unparsable or lacks
  /// `text`, matching the previous lenient rendering.
  TextDelta { text: Option<String> },
  /// `tool_started`.
  ToolStarted { name: String },
  /// `tool_finished`.
  ToolFinished { name: String },
  /// `approval_required`.
  ApprovalRequired,
  /// One of the four terminal run kinds.
  Terminal(TerminalOutcome),
  /// Any other kind, including kinds added after this client was released.
  Other,
}

/// A decoded event: raw kind plus payload for display, classified body for
/// branching.
#[derive(Clone, Debug)]
pub(crate) struct DecodedEvent {
  /// Raw daemon kind, surfaced verbatim in the client status line.
  pub(crate) kind: String,
  /// Parsed payload, or `Value::Null` when `payload_json` is not valid JSON.
  pub(crate) payload: Value,
  pub(crate) event: RunEvent,
}

impl DecodedEvent {
  /// `status` reported by run transition payloads, when present.
  pub(crate) fn status(&self) -> Option<&str> {
    self.payload.get("status").and_then(Value::as_str)
  }
}

/// Decode a wire event without ever dropping it.
pub(crate) fn decode(event: &Event) -> DecodedEvent {
  let payload: Value = serde_json::from_str(&event.payload_json).unwrap_or_default();
  let decoded = match event.kind.as_str() {
    "text_delta" => RunEvent::TextDelta {
      text: payload
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_owned),
    },
    "tool_started" => RunEvent::ToolStarted {
      name: tool_name(&payload),
    },
    "tool_finished" => RunEvent::ToolFinished {
      name: tool_name(&payload),
    },
    APPROVAL_REQUIRED => RunEvent::ApprovalRequired,
    kind => TerminalOutcome::from_kind(kind).map_or(RunEvent::Other, RunEvent::Terminal),
  };
  DecodedEvent {
    kind: event.kind.clone(),
    payload,
    event: decoded,
  }
}

/// Tool label for the transcript block.
///
/// Runtime `tool_started`/`tool_finished` payloads carry `name`; `tool` is the
/// field approvals use, kept as a fallback so a tool payload that only carries
/// `tool` renders the same label as before.
fn tool_name(payload: &Value) -> String {
  payload
    .get("name")
    .and_then(Value::as_str)
    .or_else(|| payload.get("tool").and_then(Value::as_str))
    .unwrap_or("Tool")
    .to_owned()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn event(kind: &str, payload: &str) -> Event {
    Event {
      cursor: 7,
      run_id: "run-1".into(),
      kind: kind.into(),
      payload_json: payload.into(),
      created_at_ms: 0,
    }
  }

  #[test]
  fn decodes_text_delta() {
    let decoded = decode(&event("text_delta", r#"{"text":"hello"}"#));
    assert_eq!(
      decoded.event,
      RunEvent::TextDelta {
        text: Some("hello".into())
      }
    );
    assert_eq!(decoded.kind, "text_delta");
    assert_eq!(decoded.status(), None);
  }

  #[test]
  fn keeps_text_delta_with_unparsable_payload() {
    let decoded = decode(&event("text_delta", "not json"));
    assert_eq!(decoded.event, RunEvent::TextDelta { text: None });
    assert!(decoded.payload.is_null());
    assert_eq!(decoded.payload["text"].as_str(), None);
  }

  #[test]
  fn decodes_tool_events() {
    let started = decode(&event(
      "tool_started",
      r#"{"call_id":"c1","name":"read_file","arguments":{}}"#,
    ));
    assert_eq!(
      started.event,
      RunEvent::ToolStarted {
        name: "read_file".into()
      }
    );
    let finished = decode(&event(
      "tool_finished",
      r#"{"call_id":"c1","name":"read_file","text":"ok","is_error":false}"#,
    ));
    assert_eq!(
      finished.event,
      RunEvent::ToolFinished {
        name: "read_file".into()
      }
    );
    let missing = decode(&event("tool_started", "{}"));
    assert_eq!(
      missing.event,
      RunEvent::ToolStarted {
        name: "Tool".into()
      }
    );
  }

  #[test]
  fn decodes_approval_required() {
    let decoded = decode(&event(
      "approval_required",
      r#"{"approval_id":"a1","tool":"write_file","arguments":{}}"#,
    ));
    assert_eq!(decoded.event, RunEvent::ApprovalRequired);
    assert_eq!(decoded.status(), None);
    assert!(is_approval_required("approval_required"));
    assert!(!is_approval_required("approval_decided"));
  }

  #[test]
  fn decodes_terminal_kinds() {
    for (kind, outcome) in [
      ("run_completed", TerminalOutcome::Completed),
      ("run_failed", TerminalOutcome::Failed),
      ("run_cancelled", TerminalOutcome::Cancelled),
      ("run_uncertain", TerminalOutcome::Uncertain),
    ] {
      let decoded = decode(&event(kind, r#"{"status":"terminal"}"#));
      assert_eq!(decoded.event, RunEvent::Terminal(outcome));
      assert_eq!(decoded.status(), Some("terminal"));
      assert!(is_terminal(kind));
    }
    for kind in ["run_running", "run_accepted", "run_finished", "text_delta"] {
      assert!(!is_terminal(kind));
    }
  }

  #[test]
  fn unknown_kinds_fall_back_to_other() {
    let decoded = decode(&event("future_kind", r#"{"status":"running"}"#));
    assert_eq!(decoded.event, RunEvent::Other);
    assert_eq!(decoded.kind, "future_kind");
    assert_eq!(decoded.status(), Some("running"));
  }

  #[test]
  fn run_status_transitions_are_other() {
    let decoded = decode(&event(
      "run_awaiting_approval",
      r#"{"status":"awaiting_approval"}"#,
    ));
    assert_eq!(decoded.event, RunEvent::Other);
    assert_eq!(decoded.status(), Some("awaiting_approval"));
  }
}
