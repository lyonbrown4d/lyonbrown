use agent_tool::{ApprovalDecision as ToolDecision, ApprovalResponse};

use crate::approval::PendingApproval;
use crate::transport::CommandRequest;
use crate::{
  AgentCommand, ApprovalDecision, RuntimeCommandError, RuntimePhase, UnsupportedCommand,
};

use super::actor::Actor;

impl Actor {
  pub(super) fn handle_command(&mut self, request: CommandRequest) {
    let result = match request.command.clone() {
      AgentCommand::StartTurn { text } => self.start_command(text),
      AgentCommand::SteerCurrentTurn { text } => self.steer_command(text),
      AgentCommand::QueueFollowUp { text } => self.queue_command(text),
      AgentCommand::CancelTurn => self.cancel_command(),
      AgentCommand::ForceStop => self.force_stop_command(),
      AgentCommand::RespondApproval {
        request_id,
        decision,
      } => self.respond_approval(request_id, decision),
      AgentCommand::SelectSessionModel { .. } => Err(RuntimeCommandError::Unsupported(
        UnsupportedCommand::SelectSessionModel,
      )),
      AgentCommand::StartModelDownload { .. } => Err(RuntimeCommandError::Unsupported(
        UnsupportedCommand::StartModelDownload,
      )),
      AgentCommand::CancelModelDownload { .. } => Err(RuntimeCommandError::Unsupported(
        UnsupportedCommand::CancelModelDownload,
      )),
    };
    request.respond(result);
  }

  fn start_command(&mut self, text: String) -> Result<(), RuntimeCommandError> {
    if self.projector.snapshot().phase == RuntimePhase::ModelSetupRequired {
      Err(RuntimeCommandError::ModelSetupRequired)
    } else if self.turn_task.is_some() {
      Err(RuntimeCommandError::TurnAlreadyActive)
    } else if text.trim().is_empty() {
      Err(RuntimeCommandError::EmptyText)
    } else {
      self.start_turn(text);
      Ok(())
    }
  }

  fn steer_command(&self, text: String) -> Result<(), RuntimeCommandError> {
    if self.projector.snapshot().phase == RuntimePhase::ModelSetupRequired {
      return Err(RuntimeCommandError::ModelSetupRequired);
    }
    if text.trim().is_empty() {
      return Err(RuntimeCommandError::EmptyText);
    }
    self
      .control
      .as_ref()
      .ok_or(RuntimeCommandError::NoActiveTurn)?
      .steer(text)
      .map_err(|error| RuntimeCommandError::Steering(error.to_string()))
  }

  fn queue_command(&mut self, text: String) -> Result<(), RuntimeCommandError> {
    if self.projector.snapshot().phase == RuntimePhase::ModelSetupRequired {
      return Err(RuntimeCommandError::ModelSetupRequired);
    }
    if text.trim().is_empty() {
      return Err(RuntimeCommandError::EmptyText);
    }
    self.queued.push_back(text);
    self.publish_queue();
    Ok(())
  }

  fn cancel_command(&mut self) -> Result<(), RuntimeCommandError> {
    let cancellation = self
      .cancellation
      .clone()
      .ok_or(RuntimeCommandError::NoActiveTurn)?;
    self.deny_pending();
    cancellation.cancel();
    self.projector.set_phase(RuntimePhase::Cancelling);
    self.publish();
    Ok(())
  }

  fn force_stop_command(&mut self) -> Result<(), RuntimeCommandError> {
    let task = self
      .turn_task
      .take()
      .ok_or(RuntimeCommandError::NoActiveTurn)?;
    self.deny_pending();
    self.queued.clear();
    self.projector.set_queued_follow_ups(&self.queued);
    if let Some(cancellation) = &self.cancellation {
      cancellation.cancel();
    }
    task.abort();
    self.control = None;
    self.cancellation = None;
    self.commands_open = false;
    self
      .projector
      .record_failure("turn was force-stopped before cleanup completed".into());
    self.projector.set_phase(RuntimePhase::FailedTerminal);
    self.publish();
    Ok(())
  }

  fn respond_approval(
    &mut self,
    request_id: agent_core::ToolCallId,
    decision: ApprovalDecision,
  ) -> Result<(), RuntimeCommandError> {
    if self
      .pending_approval
      .as_ref()
      .is_some_and(PendingApproval::is_closed)
    {
      self.deny_pending();
      return Err(RuntimeCommandError::NoPendingApproval);
    }
    let pending = self
      .pending_approval
      .as_ref()
      .ok_or(RuntimeCommandError::NoPendingApproval)?;
    if pending.request.request_id != request_id {
      return Err(RuntimeCommandError::ApprovalRequestMismatch {
        expected: pending.request.request_id,
        received: request_id,
      });
    }
    let pending = self
      .pending_approval
      .take()
      .expect("checked pending approval");
    pending.respond(ApprovalResponse {
      request_id,
      decision: map_decision(decision),
    });
    self.projector.clear_approval_request(request_id);
    self.publish();
    Ok(())
  }
}

fn map_decision(decision: ApprovalDecision) -> ToolDecision {
  match decision {
    ApprovalDecision::AllowOnce => ToolDecision::Allow,
    ApprovalDecision::AllowForSession => ToolDecision::AllowSession,
    ApprovalDecision::DenyOnce => ToolDecision::Deny,
    ApprovalDecision::DenyRemaining => ToolDecision::DenyAll,
  }
}
