mod activity;
mod common;
mod model_setup;

use std::time::Instant;

use agent_app::{
  ApprovalState as AppApprovalState, ModelSelection as AppModelSelection, RuntimePhase as AppPhase,
};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{
  ApprovalSnapshot, ApprovalState, ModelSelection, RuntimeEvent, RuntimePhase, RuntimeSnapshot,
};

use crate::VersionedSnapshot;
use activity::turn_to_wire;
use common::{route_to_wire, tool_preview_to_wire};
use model_setup::model_setup_to_wire;

pub fn snapshot_to_wire(versioned: VersionedSnapshot) -> RuntimeSnapshot {
  let snapshot = versioned.snapshot;
  RuntimeSnapshot {
    protocol_version: PROTOCOL_VERSION,
    revision: versioned.revision,
    phase: phase_to_wire(snapshot.phase) as i32,
    session_id: snapshot.session_id.map(|value| value.to_string()),
    turns: snapshot.turns.into_iter().map(turn_to_wire).collect(),
    queued_follow_ups: snapshot.queued_follow_ups,
    pending_approval: snapshot.approval.map(|approval| ApprovalSnapshot {
      request_id: approval.request_id.to_string(),
      tool_name: approval.tool_name,
      arguments_json: approval.arguments.to_string(),
      step_id: approval.step_id.to_string(),
      preview: approval.preview.map(tool_preview_to_wire),
      deadline_remaining_ms: approval.deadline.map(|deadline| {
        deadline
          .saturating_duration_since(Instant::now())
          .as_millis()
          .min(u128::from(u64::MAX)) as u64
      }),
      state: approval_state_to_wire(approval.state) as i32,
      approved_by: approval.approved_by,
    }),
    last_error: snapshot.last_error,
    model_selection: Some(model_selection_to_wire(snapshot.model_selection)),
    route: snapshot.route.map(route_to_wire),
    model_setup: snapshot.model_setup.map(model_setup_to_wire),
  }
}

pub(crate) fn event_to_wire(snapshot: VersionedSnapshot) -> RuntimeEvent {
  let revision = snapshot.revision;
  RuntimeEvent {
    protocol_version: PROTOCOL_VERSION,
    revision,
    snapshot: Some(snapshot_to_wire(snapshot)),
  }
}

fn model_selection_to_wire(selection: AppModelSelection) -> ModelSelection {
  let selection = match selection {
    AppModelSelection::Auto => Selection::Auto(true),
    AppModelSelection::Pinned { provider, model } => {
      Selection::Pinned(agent_protocol::v1::ModelRoute { provider, model })
    }
  };
  ModelSelection {
    selection: Some(selection),
  }
}

fn approval_state_to_wire(state: AppApprovalState) -> ApprovalState {
  match state {
    AppApprovalState::Pending => ApprovalState::Pending,
    AppApprovalState::Approved => ApprovalState::Approved,
  }
}

fn phase_to_wire(phase: AppPhase) -> RuntimePhase {
  match phase {
    AppPhase::Initializing => RuntimePhase::Initializing,
    AppPhase::ModelSetupRequired => RuntimePhase::ModelSetupRequired,
    AppPhase::Ready => RuntimePhase::Ready,
    AppPhase::Preparing => RuntimePhase::Preparing,
    AppPhase::Streaming => RuntimePhase::Streaming,
    AppPhase::AwaitingApproval => RuntimePhase::AwaitingApproval,
    AppPhase::RunningTool => RuntimePhase::RunningTool,
    AppPhase::Cancelling => RuntimePhase::Cancelling,
    AppPhase::Completed => RuntimePhase::Completed,
    AppPhase::FailedRecoverable => RuntimePhase::FailedRecoverable,
    AppPhase::FailedTerminal => RuntimePhase::FailedTerminal,
  }
}
