use agent_app::{
  ActivityPhase as AppActivityPhase, ActivityView, ContextSummary,
  ToolActivityState as AppToolState, ToolActivityView, TurnStatus as AppTurnStatus, TurnView,
};
use agent_core::{StepId, TokenUsage, ToolCallId, ToolResult, TurnId};
use agent_protocol::v1::{ActivityPhase, ToolActivityState, TurnStatus};

use super::common::{json_value, route_from_wire, step_outcome_from_wire};
use super::{SnapshotDecodeError, parse_uuid};

pub(super) fn turn_from_wire(
  wire: agent_protocol::v1::TurnSnapshot,
) -> Result<TurnView, SnapshotDecodeError> {
  Ok(TurnView {
    turn_id: TurnId(parse_uuid("turn.turn_id", wire.turn_id)?),
    user_text: wire.user_text,
    steering: wire.steering,
    assistant_text: wire.assistant_text,
    status: turn_status_from_wire(wire.status)?,
    activities: wire
      .activities
      .into_iter()
      .map(activity_from_wire)
      .collect::<Result<_, _>>()?,
  })
}

fn activity_from_wire(
  wire: agent_protocol::v1::ActivitySnapshot,
) -> Result<ActivityView, SnapshotDecodeError> {
  Ok(ActivityView {
    step_id: StepId(parse_uuid("activity.step_id", wire.step_id)?),
    phase: activity_phase_from_wire(wire.phase)?,
    route: wire.route.map(route_from_wire).transpose()?,
    context: wire.context.map(|context| ContextSummary {
      system_tokens: context.system_tokens,
      context_tokens: context.context_tokens,
      dropped: context.dropped,
    }),
    assistant_text: wire.assistant_text,
    usage: wire.usage.map(|usage| TokenUsage {
      input: usage.input,
      output: usage.output,
      cache_read: usage.cache_read,
    }),
    tools: wire
      .tools
      .into_iter()
      .map(tool_from_wire)
      .collect::<Result<_, _>>()?,
    outcome: wire.outcome.map(step_outcome_from_wire).transpose()?,
  })
}

fn tool_from_wire(
  wire: agent_protocol::v1::ToolActivity,
) -> Result<ToolActivityView, SnapshotDecodeError> {
  Ok(ToolActivityView {
    call_id: ToolCallId(parse_uuid("tool.call_id", wire.call_id)?),
    name: wire.name,
    arguments: json_value("tool.arguments_json", &wire.arguments_json)?,
    state: tool_state_from_wire(wire.state)?,
    approved_by: wire.approved_by,
    result: wire.result.map(tool_result_from_wire).transpose()?,
  })
}

fn tool_result_from_wire(
  wire: agent_protocol::v1::ToolResult,
) -> Result<ToolResult, SnapshotDecodeError> {
  Ok(ToolResult {
    invocation_id: ToolCallId(parse_uuid("tool.result.invocation_id", wire.invocation_id)?),
    ok: wire.ok,
    output: wire.output,
    truncated: wire.truncated,
    duration_ms: wire.duration_ms,
    error: wire.error,
  })
}

fn turn_status_from_wire(value: i32) -> Result<AppTurnStatus, SnapshotDecodeError> {
  match TurnStatus::try_from(value) {
    Ok(TurnStatus::Pending) => Ok(AppTurnStatus::Pending),
    Ok(TurnStatus::Running) => Ok(AppTurnStatus::Running),
    Ok(TurnStatus::Completed) => Ok(AppTurnStatus::Completed),
    Ok(TurnStatus::Cancelled) => Ok(AppTurnStatus::Cancelled),
    Ok(TurnStatus::Failed) => Ok(AppTurnStatus::Failed),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "turn.status",
      value,
    }),
  }
}

fn activity_phase_from_wire(value: i32) -> Result<AppActivityPhase, SnapshotDecodeError> {
  match ActivityPhase::try_from(value) {
    Ok(ActivityPhase::Preparing) => Ok(AppActivityPhase::Preparing),
    Ok(ActivityPhase::Streaming) => Ok(AppActivityPhase::Streaming),
    Ok(ActivityPhase::AwaitingApproval) => Ok(AppActivityPhase::AwaitingApproval),
    Ok(ActivityPhase::RunningTool) => Ok(AppActivityPhase::RunningTool),
    Ok(ActivityPhase::Superseded) => Ok(AppActivityPhase::Superseded),
    Ok(ActivityPhase::Completed) => Ok(AppActivityPhase::Completed),
    Ok(ActivityPhase::Failed) => Ok(AppActivityPhase::Failed),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "activity.phase",
      value,
    }),
  }
}

fn tool_state_from_wire(value: i32) -> Result<AppToolState, SnapshotDecodeError> {
  match ToolActivityState::try_from(value) {
    Ok(ToolActivityState::Requested) => Ok(AppToolState::Requested),
    Ok(ToolActivityState::Approved) => Ok(AppToolState::Approved),
    Ok(ToolActivityState::Running) => Ok(AppToolState::Running),
    Ok(ToolActivityState::Completed) => Ok(AppToolState::Completed),
    Ok(ToolActivityState::Failed) => Ok(AppToolState::Failed),
    _ => Err(SnapshotDecodeError::UnknownEnum {
      field: "tool.state",
      value,
    }),
  }
}
