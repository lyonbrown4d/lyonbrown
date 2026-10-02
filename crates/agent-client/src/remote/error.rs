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
  if let Ok(detail) = CommandErrorDetail::decode(status.details()) {
    if let Some(error) = epoch_error(&detail) {
      return error;
    }
    if let Some(error) = command_error(detail) {
      return AgentClientError::Command(error);
    }
  }
  coarse_error(status)
}

fn epoch_error(detail: &CommandErrorDetail) -> Option<AgentClientError> {
  let code = CommandErrorCode::try_from(detail.code).ok()?;
  if code != CommandErrorCode::EpochMismatch {
    return None;
  }
  let Context::EpochMismatch(ids) = detail.context.as_ref()? else {
    return None;
  };
  Some(AgentClientError::unavailable(
    AvailabilityKind::StaleEpoch,
    format!(
      "agent daemon epoch changed (expected {}, connected {})",
      ids.expected_server_instance_id, ids.actual_server_instance_id
    ),
  ))
}

fn command_error(detail: CommandErrorDetail) -> Option<RuntimeCommandError> {
  let code = CommandErrorCode::try_from(detail.code).ok()?;
  match code {
    CommandErrorCode::RuntimeStopped => Some(RuntimeCommandError::RuntimeStopped),
    CommandErrorCode::ModelSetupRequired => Some(RuntimeCommandError::ModelSetupRequired),
    CommandErrorCode::ModelDownloadUnavailable => {
      Some(RuntimeCommandError::ModelDownloadUnavailable)
    }
    CommandErrorCode::InvalidModelId => match detail.context? {
      Context::InvalidModelId(ids) => Some(RuntimeCommandError::InvalidModelId {
        requested: ids.requested_model_id,
        configured: ids.configured_model_id,
      }),
      _ => None,
    },
    CommandErrorCode::NoActiveModelDownload => Some(RuntimeCommandError::NoActiveModelDownload),
    CommandErrorCode::ModelDownloadOperationMismatch => match detail.context? {
      Context::ModelDownloadOperationMismatch(ids) => {
        Some(RuntimeCommandError::ModelDownloadOperationMismatch {
          expected: ids.expected_operation_id,
          received: ids.received_operation_id,
        })
      }
      _ => None,
    },
    CommandErrorCode::ModelDownloadCancellationTooLate => {
      Some(RuntimeCommandError::ModelDownloadCancellationTooLate)
    }
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
          UnsupportedCommand::StartModelDownload => AppUnsupportedCommand::StartModelDownload,
          UnsupportedCommand::CancelModelDownload => AppUnsupportedCommand::CancelModelDownload,
          UnsupportedCommand::Unspecified => return None,
        };
        Some(RuntimeCommandError::Unsupported(command))
      }
      _ => None,
    },
    CommandErrorCode::EpochMismatch => None,
    CommandErrorCode::Unspecified => None,
  }
}

fn coarse_error(status: Status) -> AgentClientError {
  match status.code() {
    Code::Unauthenticated => AgentClientError::protocol(
      ProtocolErrorKind::AuthenticationFailed,
      "authentication required",
    ),
    Code::DeadlineExceeded => {
      AgentClientError::unavailable(AvailabilityKind::Timeout, status.message())
    }
    Code::Unavailable => {
      AgentClientError::unavailable(AvailabilityKind::Disconnected, status.message())
    }
    _ => AgentClientError::protocol(ProtocolErrorKind::Unsupported, status.message()),
  }
}
