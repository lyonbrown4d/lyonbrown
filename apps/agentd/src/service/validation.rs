use agent_app::{ApprovalDecision, RuntimeCommandError, UnsupportedCommand};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::command_error_detail::Context;
use agent_protocol::v1::{
  ApprovalRequestMismatchDetail, CommandErrorCode, CommandErrorDetail, RequestMetadata,
  UnsupportedCommand as WireUnsupportedCommand,
};
use prost::Message;
use tonic::{Code, Status};

const MAX_CLIENT_INSTANCE_ID_BYTES: usize = 128;
// Command IDs are bounded opaque idempotency keys; UUID formatting is not required by v1.
const MAX_COMMAND_ID_BYTES: usize = 128;

pub(super) fn validate_metadata(
  metadata: Option<RequestMetadata>,
) -> Result<RequestMetadata, Status> {
  let metadata = metadata.ok_or_else(|| Status::invalid_argument("metadata is required"))?;
  if metadata.protocol_version != PROTOCOL_VERSION {
    return Err(Status::failed_precondition(format!(
      "unsupported protocol_version {}; server supports protocol_version {PROTOCOL_VERSION}",
      metadata.protocol_version
    )));
  }
  validate_required_bounded(
    "client_instance_id",
    &metadata.client_instance_id,
    MAX_CLIENT_INSTANCE_ID_BYTES,
  )?;
  validate_required_bounded("command_id", &metadata.command_id, MAX_COMMAND_ID_BYTES)?;
  Ok(metadata)
}

pub(super) fn validate_required_bounded(
  field: &'static str,
  value: &str,
  max_bytes: usize,
) -> Result<(), Status> {
  if value.trim().is_empty() {
    return Err(Status::invalid_argument(format!("{field} is required")));
  }
  if value.len() > max_bytes {
    return Err(Status::invalid_argument(format!(
      "{field} exceeds {max_bytes} bytes"
    )));
  }
  Ok(())
}

pub(super) fn approval_decision(value: i32) -> Result<ApprovalDecision, Status> {
  use agent_protocol::v1::ApprovalDecision as WireDecision;
  match WireDecision::try_from(value).unwrap_or(WireDecision::Unspecified) {
    WireDecision::AllowOnce => Ok(ApprovalDecision::AllowOnce),
    WireDecision::AllowForSession => Ok(ApprovalDecision::AllowForSession),
    WireDecision::DenyOnce => Ok(ApprovalDecision::DenyOnce),
    WireDecision::DenyRemaining => Ok(ApprovalDecision::DenyRemaining),
    WireDecision::Unspecified => Err(Status::invalid_argument("approval decision is required")),
  }
}

pub(super) fn map_command_error(error: RuntimeCommandError) -> Status {
  let (status_code, detail_code, context) = match &error {
    RuntimeCommandError::RuntimeStopped => {
      (Code::Unavailable, CommandErrorCode::RuntimeStopped, None)
    }
    RuntimeCommandError::TurnAlreadyActive => (
      Code::FailedPrecondition,
      CommandErrorCode::TurnAlreadyActive,
      None,
    ),
    RuntimeCommandError::NoActiveTurn => (
      Code::FailedPrecondition,
      CommandErrorCode::NoActiveTurn,
      None,
    ),
    RuntimeCommandError::EmptyText => (Code::InvalidArgument, CommandErrorCode::EmptyText, None),
    RuntimeCommandError::Steering(reason) => (
      Code::FailedPrecondition,
      CommandErrorCode::Steering,
      Some(Context::SteeringReason(reason.clone())),
    ),
    RuntimeCommandError::NoPendingApproval => (
      Code::FailedPrecondition,
      CommandErrorCode::NoPendingApproval,
      None,
    ),
    RuntimeCommandError::ApprovalRequestMismatch { expected, received } => (
      Code::FailedPrecondition,
      CommandErrorCode::ApprovalRequestMismatch,
      Some(Context::ApprovalRequestMismatch(
        ApprovalRequestMismatchDetail {
          expected_request_id: expected.to_string(),
          received_request_id: received.to_string(),
        },
      )),
    ),
    RuntimeCommandError::Unsupported(command) => (
      Code::FailedPrecondition,
      CommandErrorCode::Unsupported,
      Some(Context::UnsupportedCommand(
        unsupported_command(*command) as i32
      )),
    ),
  };
  let detail = CommandErrorDetail {
    code: detail_code as i32,
    context,
  };
  Status::with_details(
    status_code,
    error.to_string(),
    detail.encode_to_vec().into(),
  )
}

fn unsupported_command(command: UnsupportedCommand) -> WireUnsupportedCommand {
  match command {
    UnsupportedCommand::ForceStop => WireUnsupportedCommand::ForceStop,
    UnsupportedCommand::RespondApproval => WireUnsupportedCommand::RespondApproval,
    UnsupportedCommand::SelectSessionModel => WireUnsupportedCommand::SelectSessionModel,
  }
}
