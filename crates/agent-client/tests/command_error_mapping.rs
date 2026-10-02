use agent_app::{RuntimeCommandError, UnsupportedCommand as AppUnsupportedCommand};
use agent_client::{AgentClientError, AvailabilityKind, ProtocolErrorKind};
use agent_core::ToolCallId;
use agent_protocol::v1::command_error_detail::Context;
use agent_protocol::v1::{
  ApprovalRequestMismatchDetail, CommandErrorCode, CommandErrorDetail, EpochMismatchDetail,
  InvalidModelIdDetail, ModelDownloadOperationMismatchDetail,
  UnsupportedCommand as WireUnsupportedCommand,
};
use prost::Message;
use tonic::{Code, Status};
use uuid::Uuid;

#[path = "../src/remote/error.rs"]
mod error;

fn mapped(grpc_code: Code, detail: CommandErrorDetail) -> AgentClientError {
  error::status_error(Status::with_details(
    grpc_code,
    "human message must not select the error kind",
    detail.encode_to_vec().into(),
  ))
}

fn detail(code: CommandErrorCode) -> CommandErrorDetail {
  CommandErrorDetail {
    code: code as i32,
    context: None,
  }
}

#[test]
fn known_command_error_details_restore_runtime_errors() {
  let expected_id = ToolCallId(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap());
  let received_id = ToolCallId(Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap());
  let cases = [
    (
      Code::Unavailable,
      detail(CommandErrorCode::RuntimeStopped),
      RuntimeCommandError::RuntimeStopped,
    ),
    (
      Code::FailedPrecondition,
      detail(CommandErrorCode::ModelSetupRequired),
      RuntimeCommandError::ModelSetupRequired,
    ),
    (
      Code::FailedPrecondition,
      detail(CommandErrorCode::ModelDownloadUnavailable),
      RuntimeCommandError::ModelDownloadUnavailable,
    ),
    (
      Code::InvalidArgument,
      CommandErrorDetail {
        code: CommandErrorCode::InvalidModelId as i32,
        context: Some(Context::InvalidModelId(InvalidModelIdDetail {
          requested_model_id: "other".into(),
          configured_model_id: "configured".into(),
        })),
      },
      RuntimeCommandError::InvalidModelId {
        requested: "other".into(),
        configured: "configured".into(),
      },
    ),
    (
      Code::FailedPrecondition,
      detail(CommandErrorCode::NoActiveModelDownload),
      RuntimeCommandError::NoActiveModelDownload,
    ),
    (
      Code::FailedPrecondition,
      CommandErrorDetail {
        code: CommandErrorCode::ModelDownloadOperationMismatch as i32,
        context: Some(Context::ModelDownloadOperationMismatch(
          ModelDownloadOperationMismatchDetail {
            expected_operation_id: "current".into(),
            received_operation_id: "stale".into(),
          },
        )),
      },
      RuntimeCommandError::ModelDownloadOperationMismatch {
        expected: "current".into(),
        received: "stale".into(),
      },
    ),
    (
      Code::FailedPrecondition,
      detail(CommandErrorCode::ModelDownloadCancellationTooLate),
      RuntimeCommandError::ModelDownloadCancellationTooLate,
    ),
    (
      Code::FailedPrecondition,
      detail(CommandErrorCode::TurnAlreadyActive),
      RuntimeCommandError::TurnAlreadyActive,
    ),
    (
      Code::FailedPrecondition,
      detail(CommandErrorCode::NoActiveTurn),
      RuntimeCommandError::NoActiveTurn,
    ),
    (
      Code::InvalidArgument,
      detail(CommandErrorCode::EmptyText),
      RuntimeCommandError::EmptyText,
    ),
    (
      Code::FailedPrecondition,
      CommandErrorDetail {
        code: CommandErrorCode::Steering as i32,
        context: Some(Context::SteeringReason("safe boundary rejected".into())),
      },
      RuntimeCommandError::Steering("safe boundary rejected".into()),
    ),
    (
      Code::FailedPrecondition,
      detail(CommandErrorCode::NoPendingApproval),
      RuntimeCommandError::NoPendingApproval,
    ),
    (
      Code::FailedPrecondition,
      CommandErrorDetail {
        code: CommandErrorCode::ApprovalRequestMismatch as i32,
        context: Some(Context::ApprovalRequestMismatch(
          ApprovalRequestMismatchDetail {
            expected_request_id: expected_id.to_string(),
            received_request_id: received_id.to_string(),
          },
        )),
      },
      RuntimeCommandError::ApprovalRequestMismatch {
        expected: expected_id,
        received: received_id,
      },
    ),
    (
      Code::FailedPrecondition,
      CommandErrorDetail {
        code: CommandErrorCode::Unsupported as i32,
        context: Some(Context::UnsupportedCommand(
          WireUnsupportedCommand::SelectSessionModel as i32,
        )),
      },
      RuntimeCommandError::Unsupported(AppUnsupportedCommand::SelectSessionModel),
    ),
  ];

  for (grpc_code, detail, expected) in cases {
    assert_eq!(
      mapped(grpc_code, detail),
      AgentClientError::Command(expected)
    );
  }
}

#[test]
fn unknown_or_invalid_details_fall_back_without_parsing_the_message() {
  let malformed = Status::with_details(
    Code::FailedPrecondition,
    "a turn is already active",
    vec![0xff].into(),
  );
  let unknown = mapped(
    Code::FailedPrecondition,
    CommandErrorDetail {
      code: 999,
      context: None,
    },
  );
  let missing_context = mapped(Code::FailedPrecondition, detail(CommandErrorCode::Steering));
  let invalid_id = mapped(
    Code::FailedPrecondition,
    CommandErrorDetail {
      code: CommandErrorCode::ApprovalRequestMismatch as i32,
      context: Some(Context::ApprovalRequestMismatch(
        ApprovalRequestMismatchDetail {
          expected_request_id: "not-a-uuid".into(),
          received_request_id: "also-not-a-uuid".into(),
        },
      )),
    },
  );

  for error in [
    error::status_error(malformed),
    unknown,
    missing_context,
    invalid_id,
  ] {
    assert!(matches!(error, AgentClientError::Protocol { .. }));
    assert!(!matches!(error, AgentClientError::Command(_)));
  }
}

#[test]
fn legacy_statuses_without_details_keep_the_coarse_mapping() {
  assert!(matches!(
    error::status_error(Status::unavailable("legacy server")),
    AgentClientError::Unavailable {
      kind: AvailabilityKind::Disconnected,
      ..
    }
  ));
  assert!(matches!(
    error::status_error(Status::deadline_exceeded("legacy timeout")),
    AgentClientError::Unavailable {
      kind: AvailabilityKind::Timeout,
      ..
    }
  ));
  assert!(matches!(
    error::status_error(Status::failed_precondition("a turn is already active")),
    AgentClientError::Protocol {
      kind: ProtocolErrorKind::Unsupported,
      ..
    }
  ));
}

#[test]
fn unauthenticated_status_has_a_stable_non_retryable_kind() {
  let secret = "Bearer secret-must-not-escape";
  let mapped = error::status_error(Status::unauthenticated(secret));

  assert_eq!(
    mapped,
    AgentClientError::Protocol {
      kind: ProtocolErrorKind::AuthenticationFailed,
      message: "authentication required".into(),
    }
  );
  assert!(!mapped.to_string().contains(secret));
}

#[test]
fn epoch_mismatch_detail_maps_without_parsing_status_message() {
  let error = mapped(
    Code::FailedPrecondition,
    CommandErrorDetail {
      code: CommandErrorCode::EpochMismatch as i32,
      context: Some(Context::EpochMismatch(EpochMismatchDetail {
        expected_server_instance_id: "daemon-a".into(),
        actual_server_instance_id: "daemon-b".into(),
      })),
    },
  );

  assert_eq!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::StaleEpoch,
      message: "agent daemon epoch changed (expected daemon-a, connected daemon-b)".into(),
    }
  );
}
