use agent_app::{
  AgentClientError, AvailabilityKind, ProtocolErrorKind, RuntimeCommandError,
  UnsupportedCommand as AppUnsupportedCommand,
};
use agent_core::ToolCallId;
use agent_protocol::v1::command_error_detail::Context;
use agent_protocol::v1::{CommandErrorCode, CommandErrorDetail, UnsupportedCommand};
use prost::Message;
use tonic::{Code, Status};
use uuid::Uuid;

pub(super) fn status_error(status: Status) -> AgentClientError {
  if let Ok(detail) = CommandErrorDetail::decode(status.details())
    && let Some(error) = command_error(detail)
  {
    return AgentClientError::Command(error);
  }
  coarse_error(status)
}

fn command_error(detail: CommandErrorDetail) -> Option<RuntimeCommandError> {
  let code = CommandErrorCode::try_from(detail.code).ok()?;
  match code {
    CommandErrorCode::RuntimeStopped => Some(RuntimeCommandError::RuntimeStopped),
    CommandErrorCode::TurnAlreadyActive => Some(RuntimeCommandError::TurnAlreadyActive),
    CommandErrorCode::NoActiveTurn => Some(RuntimeCommandError::NoActiveTurn),
    CommandErrorCode::EmptyText => Some(RuntimeCommandError::EmptyText),
    CommandErrorCode::Steering => match detail.context? {
      Context::SteeringReason(reason) => Some(RuntimeCommandError::Steering(reason)),
      _ => None,
    },
    CommandErrorCode::NoPendingApproval => Some(RuntimeCommandError::NoPendingApproval),
    CommandErrorCode::ApprovalRequestMismatch => match detail.context? {
      Context::ApprovalRequestMismatch(ids) => {
        let expected = ToolCallId(Uuid::parse_str(&ids.expected_request_id).ok()?);
        let received = ToolCallId(Uuid::parse_str(&ids.received_request_id).ok()?);
        Some(RuntimeCommandError::ApprovalRequestMismatch { expected, received })
      }
      _ => None,
    },
    CommandErrorCode::Unsupported => match detail.context? {
      Context::UnsupportedCommand(command) => {
        let command = match UnsupportedCommand::try_from(command).ok()? {
          UnsupportedCommand::ForceStop => AppUnsupportedCommand::ForceStop,
          UnsupportedCommand::RespondApproval => AppUnsupportedCommand::RespondApproval,
          UnsupportedCommand::SelectSessionModel => AppUnsupportedCommand::SelectSessionModel,
          UnsupportedCommand::Unspecified => return None,
        };
        Some(RuntimeCommandError::Unsupported(command))
      }
      _ => None,
    },
    CommandErrorCode::Unspecified => None,
  }
}

fn coarse_error(status: Status) -> AgentClientError {
  match status.code() {
    Code::DeadlineExceeded => {
      AgentClientError::unavailable(AvailabilityKind::Timeout, status.message())
    }
    Code::Unavailable => {
      AgentClientError::unavailable(AvailabilityKind::Disconnected, status.message())
    }
    _ => AgentClientError::protocol(ProtocolErrorKind::Unsupported, status.message()),
  }
}
