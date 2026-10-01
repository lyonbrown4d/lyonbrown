mod activity;
mod common;

use std::time::{Duration, Instant};

use agent_app::{
  ApprovalState as AppApprovalState, ApprovalView, ModelSelection as AppModelSelection,
  RuntimePhase as AppPhase, RuntimeSnapshot as AppSnapshot,
};
use agent_core::{SessionId, StepId, ToolCallId};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{ApprovalState, RuntimePhase, RuntimeSnapshot};
use uuid::Uuid;

use activity::turn_from_wire;
use common::{json_value, route_from_wire, tool_preview_from_wire};

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedSnapshot {
  pub revision: u64,
  pub snapshot: AppSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotDecodeError {
  #[error("unsupported snapshot protocol version {0}")]
  ProtocolVersion(u32),
  #[error("missing required snapshot field: {0}")]
  Missing(&'static str),
  #[error("unknown {field} enum value {value}")]
  UnknownEnum { field: &'static str, value: i32 },
  #[error("invalid UUID in {field}: {value}")]
  InvalidUuid { field: &'static str, value: String },
  #[error("invalid JSON in {field}: {message}")]
  InvalidJson {
    field: &'static str,
    message: String,
  },
  #[error("invalid model selection")]
  ModelSelection,
}

pub fn decode_snapshot(wire: RuntimeSnapshot) -> Result<DecodedSnapshot, SnapshotDecodeError> {
  if wire.protocol_version != PROTOCOL_VERSION {
    return Err(SnapshotDecodeError::ProtocolVersion(wire.protocol_version));
  }
  let snapshot = AppSnapshot {
    phase: phase_from_wire(wire.phase)?,
    session_id: wire
      .session_id
      .map(|value| parse_uuid("session_id", value).map(SessionId))
      .transpose()?,
    turns: wire
      .turns
      .into_iter()
      .map(turn_from_wire)
      .collect::<Result<_, _>>()?,
    approval: wire.pending_approval.map(approval_from_wire).transpose()?,
    queued_follow_ups: wire.queued_follow_ups,
    model_selection: model_selection_from_wire(
      wire
        .model_selection
        .ok_or(SnapshotDecodeError::Missing("model_selection"))?,
    )?,
    route: wire.route.map(route_from_wire).transpose()?,
    last_error: wire.last_error,
  };
  Ok(DecodedSnapshot {
    revision: wire.revision,
    snapshot,
  })
}

fn approval_from_wire(
  wire: agent_protocol::v1::ApprovalSnapshot,
) -> Result<ApprovalView, SnapshotDecodeError> {
  Ok(ApprovalView {
    request_id: ToolCallId(parse_uuid("approval.request_id", wire.request_id)?),
    step_id: StepId(parse_uuid("approval.step_id", wire.step_id)?),
    tool_name: wire.tool_name,
    arguments: json_value("approval.arguments_json", &wire.arguments_json)?,
    preview: wire.preview.map(tool_preview_from_wire).transpose()?,
    deadline: wire
      .deadline_remaining_ms
      .map(|remaining| Instant::now() + Duration::from_millis(remaining)),
    state: approval_state_from_wire(wire.state)?,
    approved_by: wire.approved_by,
  })
}

fn model_selection_from_wire(
  wire: agent_protocol::v1::ModelSelection,
) -> Result<AppModelSelection, SnapshotDecodeError> {
  match wire.selection {
    Some(Selection::Auto(true)) => Ok(AppModelSelection::Auto),
    Some(Selection::Pinned(route)) => Ok(AppModelSelection::Pinned {
      provider: route.provider,
      model: route.model,
    }),
    _ => Err(SnapshotDecodeError::ModelSelection),
  }
}

fn phase_from_wire(value: i32) -> Result<AppPhase, SnapshotDecodeError> {
  match RuntimePhase::try_from(value) {
    Ok(RuntimePhase::Initializing) => Ok(AppPhase::Initializing),
    Ok(RuntimePhase::ModelSetupRequired) => Ok(AppPhase::ModelSetupRequired),
    Ok(RuntimePhase::Ready) => Ok(AppPhase::Ready),
    Ok(RuntimePhase::Preparing) => Ok(AppPhase::Preparing),
    Ok(RuntimePhase::Streaming) => Ok(AppPhase::Streaming),
    Ok(RuntimePhase::AwaitingApproval) => Ok(AppPhase::AwaitingApproval),
    Ok(RuntimePhase::RunningTool) => Ok(AppPhase::RunningTool),
    Ok(RuntimePhase::Cancelling) => Ok(AppPhase::Cancelling),
    Ok(RuntimePhase::Completed) => Ok(AppPhase::Completed),
    Ok(RuntimePhase::FailedRecoverable) => Ok(AppPhase::FailedRecoverable),
    Ok(RuntimePhase::FailedTerminal) => Ok(AppPhase::FailedTerminal),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "phase",
      value,
    }),
  }
}

fn approval_state_from_wire(value: i32) -> Result<AppApprovalState, SnapshotDecodeError> {
  match ApprovalState::try_from(value) {
    Ok(ApprovalState::Pending) => Ok(AppApprovalState::Pending),
    Ok(ApprovalState::Approved) => Ok(AppApprovalState::Approved),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "approval.state",
      value,
    }),
  }
}

pub(super) fn parse_uuid(field: &'static str, value: String) -> Result<Uuid, SnapshotDecodeError> {
  Uuid::parse_str(&value).map_err(|_| SnapshotDecodeError::InvalidUuid { field, value })
}
