use agent_core::{StepId, ToolCallId, ToolInvocation, ToolResult, TurnId};
use agent_tool::ApprovalRequest;

use crate::{
  ActivityPhase, ApprovalState, ApprovalView, RuntimePhase, ToolActivityState, ToolActivityView,
  TurnStatus,
};

use super::Projector;

impl Projector {
  pub(crate) fn show_approval_request(&mut self, request: &ApprovalRequest) -> bool {
    let Some(approval) = self
      .snapshot
      .approval
      .as_mut()
      .filter(|approval| approval.request_id == request.request_id)
    else {
      return false;
    };
    approval.preview = Some(request.preview.clone());
    approval.deadline = request.deadline;
    approval.state = ApprovalState::Pending;
    self.snapshot.phase = RuntimePhase::AwaitingApproval;
    true
  }

  pub(crate) fn clear_approval_request(&mut self, request_id: ToolCallId) {
    if self
      .snapshot
      .approval
      .as_ref()
      .is_some_and(|approval| approval.request_id == request_id)
    {
      self.snapshot.approval = None;
    }
  }

  pub(super) fn request_tool(&mut self, step_id: StepId, invocation: ToolInvocation) {
    let approval = ApprovalView {
      request_id: invocation.id,
      step_id,
      tool_name: invocation.name.clone(),
      arguments: invocation.arguments.clone(),
      preview: None,
      deadline: None,
      state: ApprovalState::Pending,
      approved_by: None,
    };
    if let Some(activity) = self.activity_mut(step_id) {
      activity.phase = ActivityPhase::AwaitingApproval;
      activity.tools.push(ToolActivityView {
        call_id: invocation.id,
        name: invocation.name,
        arguments: invocation.arguments,
        state: ToolActivityState::Requested,
        approved_by: None,
        result: None,
      });
    }
    self.snapshot.approval = Some(approval);
    self.snapshot.phase = RuntimePhase::AwaitingApproval;
  }

  pub(super) fn approve_tool(&mut self, step_id: StepId, call_id: ToolCallId, by: String) {
    if let Some(tool) = self.tool_mut(step_id, call_id) {
      tool.state = ToolActivityState::Approved;
      tool.approved_by = Some(by.clone());
    }
    if let Some(approval) = self
      .snapshot
      .approval
      .as_mut()
      .filter(|approval| approval.request_id == call_id)
    {
      approval.state = ApprovalState::Approved;
      approval.approved_by = Some(by);
    }
  }

  pub(super) fn start_tool(&mut self, step_id: StepId, call_id: ToolCallId) {
    if let Some(tool) = self.tool_mut(step_id, call_id) {
      tool.state = ToolActivityState::Running;
    }
    if let Some(activity) = self.activity_mut(step_id) {
      activity.phase = ActivityPhase::RunningTool;
    }
    if self
      .snapshot
      .approval
      .as_ref()
      .is_some_and(|approval| approval.request_id == call_id)
    {
      self.snapshot.approval = None;
    }
    self.snapshot.phase = RuntimePhase::RunningTool;
  }

  pub(super) fn complete_tool(&mut self, step_id: StepId, result: ToolResult) {
    let call_id = result.invocation_id;
    let succeeded = result.ok;
    if let Some(tool) = self.tool_mut(step_id, call_id) {
      tool.state = if succeeded {
        ToolActivityState::Completed
      } else {
        ToolActivityState::Failed
      };
      tool.result = Some(result);
    }
    if let Some(activity) = self.activity_mut(step_id) {
      activity.phase = ActivityPhase::Preparing;
    }
    if self
      .snapshot
      .approval
      .as_ref()
      .is_some_and(|approval| approval.request_id == call_id)
    {
      self.snapshot.approval = None;
    }
    self.snapshot.phase = RuntimePhase::Preparing;
  }

  pub(super) fn complete_turn(&mut self, turn_id: TurnId, answer: String) {
    if let Some(turn) = self.turn_mut(turn_id) {
      // Deltas are an in-progress rendering of this answer, not an additional message.
      turn.assistant_text = answer;
      turn.status = TurnStatus::Completed;
    }
    self.snapshot.approval = None;
    self.snapshot.phase = RuntimePhase::Completed;
  }

  pub(super) fn cancel_turn(&mut self, turn_id: TurnId) {
    if let Some(turn) = self.turn_mut(turn_id) {
      turn.status = TurnStatus::Cancelled;
    }
    self.snapshot.approval = None;
    self.snapshot.phase = RuntimePhase::Ready;
  }

  pub(crate) fn cancel_active_turn(&mut self) {
    let turn_id = self
      .snapshot
      .turns
      .iter()
      .rev()
      .find(|turn| matches!(turn.status, TurnStatus::Pending | TurnStatus::Running))
      .map(|turn| turn.turn_id);
    if let Some(turn_id) = turn_id {
      self.cancel_turn(turn_id);
    }
  }

  pub(super) fn record_error(&mut self, message: String) {
    if let Some(turn) = self
      .snapshot
      .turns
      .iter_mut()
      .rev()
      .find(|turn| matches!(turn.status, TurnStatus::Pending | TurnStatus::Running))
    {
      turn.status = TurnStatus::Failed;
      if let Some(activity) = turn.activities.last_mut() {
        activity.phase = ActivityPhase::Failed;
      }
    }
    self.snapshot.last_error = Some(message);
    self.snapshot.approval = None;
    self.snapshot.phase = RuntimePhase::FailedRecoverable;
  }

  fn tool_mut(&mut self, step_id: StepId, call_id: ToolCallId) -> Option<&mut ToolActivityView> {
    self
      .activity_mut(step_id)?
      .tools
      .iter_mut()
      .find(|tool| tool.call_id == call_id)
  }
}
