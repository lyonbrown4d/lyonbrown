use agent_app::{
  ActivityPhase as AppActivityPhase, ActivityView, ToolActivityState as AppToolState,
  ToolActivityView, TurnStatus as AppTurnStatus, TurnView,
};
use agent_protocol::v1::{
  ActivityPhase, ActivitySnapshot, ContextSummary, TokenUsage, ToolActivity, ToolActivityState,
  TurnSnapshot, TurnStatus,
};

use super::common::{route_to_wire, step_outcome_to_wire, tool_result_to_wire};

pub(super) fn turn_to_wire(turn: TurnView) -> TurnSnapshot {
  TurnSnapshot {
    turn_id: turn.turn_id.to_string(),
    user_text: turn.user_text,
    assistant_text: turn.assistant_text,
    status: turn_status_to_wire(turn.status) as i32,
    steering: turn.steering,
    activities: turn.activities.into_iter().map(activity_to_wire).collect(),
  }
}

fn activity_to_wire(activity: ActivityView) -> ActivitySnapshot {
  ActivitySnapshot {
    step_id: activity.step_id.to_string(),
    phase: activity_phase_to_wire(activity.phase) as i32,
    route: activity.route.map(route_to_wire),
    context: activity.context.map(|context| ContextSummary {
      system_tokens: context.system_tokens,
      context_tokens: context.context_tokens,
      dropped: context.dropped,
    }),
    assistant_text: activity.assistant_text,
    usage: activity.usage.map(|usage| TokenUsage {
      input: usage.input,
      output: usage.output,
      cache_read: usage.cache_read,
    }),
    tools: activity.tools.into_iter().map(tool_to_wire).collect(),
    outcome: activity.outcome.map(step_outcome_to_wire),
  }
}

fn tool_to_wire(tool: ToolActivityView) -> ToolActivity {
  ToolActivity {
    call_id: tool.call_id.to_string(),
    name: tool.name,
    arguments_json: tool.arguments.to_string(),
    state: tool_state_to_wire(tool.state) as i32,
    approved_by: tool.approved_by,
    result: tool.result.map(tool_result_to_wire),
  }
}

fn turn_status_to_wire(status: AppTurnStatus) -> TurnStatus {
  match status {
    AppTurnStatus::Pending => TurnStatus::Pending,
    AppTurnStatus::Running => TurnStatus::Running,
    AppTurnStatus::Completed => TurnStatus::Completed,
    AppTurnStatus::Cancelled => TurnStatus::Cancelled,
    AppTurnStatus::Failed => TurnStatus::Failed,
  }
}

fn activity_phase_to_wire(phase: AppActivityPhase) -> ActivityPhase {
  match phase {
    AppActivityPhase::Preparing => ActivityPhase::Preparing,
    AppActivityPhase::Streaming => ActivityPhase::Streaming,
    AppActivityPhase::AwaitingApproval => ActivityPhase::AwaitingApproval,
    AppActivityPhase::RunningTool => ActivityPhase::RunningTool,
    AppActivityPhase::Superseded => ActivityPhase::Superseded,
    AppActivityPhase::Completed => ActivityPhase::Completed,
    AppActivityPhase::Failed => ActivityPhase::Failed,
  }
}

fn tool_state_to_wire(state: AppToolState) -> ToolActivityState {
  match state {
    AppToolState::Requested => ToolActivityState::Requested,
    AppToolState::Approved => ToolActivityState::Approved,
    AppToolState::Running => ToolActivityState::Running,
    AppToolState::Completed => ToolActivityState::Completed,
    AppToolState::Failed => ToolActivityState::Failed,
  }
}
