use super::{AGENT_EVENT_KIND, AgentEvent};
use crate::RunStatus;
use serde::{Deserialize, Serialize, de::Error};

/// Unknown kinds retain only their name, never unreviewed payload data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DecodedEvent {
  Known(AgentEvent),
  Unknown { kind: String },
}

/// Decode the current contract or a current daemon run-status event.
///
/// Unknown outer kinds are ignored without parsing their payload. Future tagged
/// variants retain their tag only. A malformed known payload always returns an
/// error; it must not be treated as an unknown event or fabricated UI state.
pub fn decode(kind: &str, payload_json: &str) -> Result<DecodedEvent, serde_json::Error> {
  if kind == AGENT_EVENT_KIND {
    return decode_tagged(payload_json);
  }
  if let Some(status) = run_status(kind) {
    return decode_status(status, payload_json).map(DecodedEvent::Known);
  }
  Ok(DecodedEvent::Unknown { kind: kind.into() })
}

fn decode_tagged(payload_json: &str) -> Result<DecodedEvent, serde_json::Error> {
  #[derive(Deserialize)]
  struct Header {
    #[serde(rename = "type")]
    kind: String,
  }
  let header: Header = serde_json::from_str(payload_json)?;
  if matches!(
    header.kind.as_str(),
    "phase_changed"
      | "text_delta"
      | "thinking_delta"
      | "tool_started"
      | "tool_finished"
      | "run_finished"
      | "compaction"
      | "usage_updated"
      | "queue_updated"
      | "run_state_changed"
  ) {
    serde_json::from_str(payload_json).map(DecodedEvent::Known)
  } else {
    Ok(DecodedEvent::Unknown { kind: header.kind })
  }
}

fn run_status(kind: &str) -> Option<RunStatus> {
  match kind.strip_prefix("run_")? {
    "accepted" => Some(RunStatus::Accepted),
    "running" => Some(RunStatus::Running),
    "awaiting_approval" => Some(RunStatus::AwaitingApproval),
    "cancel_requested" => Some(RunStatus::CancelRequested),
    "completed" => Some(RunStatus::Completed),
    "failed" => Some(RunStatus::Failed),
    "cancelled" => Some(RunStatus::Cancelled),
    "uncertain" => Some(RunStatus::Uncertain),
    _ => None,
  }
}

fn decode_status(status: RunStatus, payload_json: &str) -> Result<AgentEvent, serde_json::Error> {
  #[derive(Deserialize)]
  struct StatusPayload {
    status: Option<RunStatus>,
    #[serde(default)]
    error: String,
  }
  let payload: StatusPayload = serde_json::from_str(payload_json)?;
  if payload.status.is_some_and(|reported| reported != status) {
    return Err(serde_json::Error::custom(
      "event kind and run status differ",
    ));
  }
  Ok(AgentEvent::RunStateChanged {
    status,
    error: payload.error,
  })
}
