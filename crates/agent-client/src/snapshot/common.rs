use std::path::PathBuf;

use agent_app::{ModelFallbackView, ModelRoute, RouteSummary};
use agent_core::{
  RiskLevel as AppRisk, SandboxStatus as AppSandbox, SecretRef, StepOutcome as AppOutcome,
  StopReason as AppStopReason, ToolCallId, ToolPreview as AppPreview,
};
use agent_protocol::v1::{RiskLevel, SandboxStatus, StopReasonKind};

use super::{SnapshotDecodeError, parse_uuid};

pub(super) fn json_value(
  field: &'static str,
  value: &str,
) -> Result<serde_json::Value, SnapshotDecodeError> {
  serde_json::from_str(value).map_err(|error| SnapshotDecodeError::InvalidJson {
    field,
    message: error.to_string(),
  })
}

pub(super) fn route_from_wire(
  wire: agent_protocol::v1::RouteSummary,
) -> Result<RouteSummary, SnapshotDecodeError> {
  Ok(RouteSummary {
    selected: model_route_from_wire(
      wire
        .selected
        .ok_or(SnapshotDecodeError::Missing("route.selected"))?,
    ),
    reason: wire.reason,
    fallbacks: wire
      .fallbacks
      .into_iter()
      .map(|fallback| {
        Ok(ModelFallbackView {
          from: model_route_from_wire(
            fallback
              .from
              .ok_or(SnapshotDecodeError::Missing("route.fallback.from"))?,
          ),
          to: model_route_from_wire(
            fallback
              .to
              .ok_or(SnapshotDecodeError::Missing("route.fallback.to"))?,
          ),
          reason: fallback.reason,
        })
      })
      .collect::<Result<_, _>>()?,
  })
}

fn model_route_from_wire(wire: agent_protocol::v1::ModelRoute) -> ModelRoute {
  ModelRoute {
    provider: wire.provider,
    model: wire.model,
  }
}

pub(super) fn tool_preview_from_wire(
  wire: agent_protocol::v1::ToolPreview,
) -> Result<AppPreview, SnapshotDecodeError> {
  Ok(AppPreview {
    tool: wire.tool,
    action: wire.action,
    target: wire.target,
    impact: wire.impact,
    risk: risk_from_wire(wire.risk)?,
    sandbox: sandbox_from_wire(wire.sandbox)?,
    working_directory: wire.working_directory.map(PathBuf::from),
    required_secrets: wire.required_secrets.into_iter().map(SecretRef).collect(),
  })
}

pub(super) fn step_outcome_from_wire(
  wire: agent_protocol::v1::StepOutcome,
) -> Result<AppOutcome, SnapshotDecodeError> {
  Ok(AppOutcome {
    stop_reason: stop_reason_from_wire(
      wire
        .stop_reason
        .ok_or(SnapshotDecodeError::Missing("outcome.stop_reason"))?,
    )?,
    text: wire.text,
    tool_calls: wire
      .tool_call_ids
      .into_iter()
      .map(|value| parse_uuid("outcome.tool_call_ids", value).map(ToolCallId))
      .collect::<Result<_, _>>()?,
    escalated: wire.escalated,
  })
}

fn stop_reason_from_wire(
  wire: agent_protocol::v1::StopReason,
) -> Result<AppStopReason, SnapshotDecodeError> {
  match StopReasonKind::try_from(wire.kind) {
    Ok(StopReasonKind::EndTurn) => Ok(AppStopReason::EndTurn),
    Ok(StopReasonKind::ToolUse) => Ok(AppStopReason::ToolUse),
    Ok(StopReasonKind::MaxTokens) => Ok(AppStopReason::MaxTokens),
    Ok(StopReasonKind::Stop) => Ok(AppStopReason::Stop),
    Ok(StopReasonKind::Error) => Ok(AppStopReason::Error(wire.error.unwrap_or_default())),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "outcome.stop_reason",
      value: wire.kind,
    }),
  }
}

fn risk_from_wire(value: i32) -> Result<AppRisk, SnapshotDecodeError> {
  match RiskLevel::try_from(value) {
    Ok(RiskLevel::Read) => Ok(AppRisk::Read),
    Ok(RiskLevel::Write) => Ok(AppRisk::Write),
    Ok(RiskLevel::Execute) => Ok(AppRisk::Execute),
    Ok(RiskLevel::Network) => Ok(AppRisk::Network),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "preview.risk",
      value,
    }),
  }
}

fn sandbox_from_wire(value: i32) -> Result<AppSandbox, SnapshotDecodeError> {
  match SandboxStatus::try_from(value) {
    Ok(SandboxStatus::WorkspaceRestricted) => Ok(AppSandbox::WorkspaceRestricted),
    Ok(SandboxStatus::HostUnsandboxed) => Ok(AppSandbox::HostUnsandboxed),
    Ok(SandboxStatus::Unknown) => Ok(AppSandbox::Unknown),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "preview.sandbox",
      value,
    }),
  }
}
