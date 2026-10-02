mod support;

use std::sync::Arc;

use agent_app::{AgentCommand, RuntimeCommandError, RuntimeSnapshot};
use agent_core::ToolCallId;
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::command_error_detail::Context;
use agent_protocol::v1::{
  ApprovalRequestMismatchDetail, CommandErrorCode, CommandErrorDetail, InvalidModelIdDetail,
  ModelDownloadOperationMismatchDetail, RequestMetadata, StartTurnRequest,
  UnsupportedCommand as WireUnsupportedCommand,
};
use agentd::{RuntimeHost, VersionedSnapshot};
use async_trait::async_trait;
use prost::Message;
use tokio::sync::watch;
use tonic::Code;

use support::spawn_authenticated;

#[derive(Clone)]
struct RejectingHost {
  error: RuntimeCommandError,
  snapshots: watch::Sender<VersionedSnapshot>,
}

impl RejectingHost {
  fn new(error: RuntimeCommandError) -> Self {
    let (snapshots, _) = watch::channel(VersionedSnapshot::new(1, RuntimeSnapshot::default()));
    Self { error, snapshots }
  }
}

#[async_trait]
impl RuntimeHost for RejectingHost {
  async fn execute(&self, _command: AgentCommand) -> Result<(), RuntimeCommandError> {
    Err(self.error.clone())
  }

  fn snapshot(&self) -> VersionedSnapshot {
    self.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<VersionedSnapshot> {
    self.snapshots.subscribe()
  }
}

fn request(command_id: usize, server_instance_id: &str) -> StartTurnRequest {
  StartTurnRequest {
    metadata: Some(RequestMetadata {
      protocol_version: PROTOCOL_VERSION,
      client_instance_id: "typed-error-test".into(),
      command_id: format!("command-{command_id}"),
      expected_server_instance_id: server_instance_id.into(),
    }),
    text: "hello".into(),
  }
}

async fn status_for(error: RuntimeCommandError, command_id: usize) -> tonic::Status {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_authenticated(listener, Arc::new(RejectingHost::new(error)));
  let request = request(command_id, &server.server_instance_id());
  server
    .raw_client()
    .await
    .start_turn(request)
    .await
    .unwrap_err()
}

#[tokio::test]
async fn every_runtime_command_error_has_a_typed_wire_detail() {
  use agent_app::UnsupportedCommand as AppUnsupportedCommand;

  let expected_id = ToolCallId::new();
  let received_id = ToolCallId::new();
  let cases = [
    (
      RuntimeCommandError::RuntimeStopped,
      Code::Unavailable,
      CommandErrorCode::RuntimeStopped,
      None,
    ),
    (
      RuntimeCommandError::ModelSetupRequired,
      Code::FailedPrecondition,
      CommandErrorCode::ModelSetupRequired,
      None,
    ),
    (
      RuntimeCommandError::ModelDownloadUnavailable,
      Code::FailedPrecondition,
      CommandErrorCode::ModelDownloadUnavailable,
      None,
    ),
    (
      RuntimeCommandError::InvalidModelId {
        requested: "other".into(),
        configured: "configured".into(),
      },
      Code::InvalidArgument,
      CommandErrorCode::InvalidModelId,
      Some(Context::InvalidModelId(InvalidModelIdDetail {
        requested_model_id: "other".into(),
        configured_model_id: "configured".into(),
      })),
    ),
    (
      RuntimeCommandError::NoActiveModelDownload,
      Code::FailedPrecondition,
      CommandErrorCode::NoActiveModelDownload,
      None,
    ),
    (
      RuntimeCommandError::ModelDownloadOperationMismatch {
        expected: "current".into(),
        received: "stale".into(),
      },
      Code::FailedPrecondition,
      CommandErrorCode::ModelDownloadOperationMismatch,
      Some(Context::ModelDownloadOperationMismatch(
        ModelDownloadOperationMismatchDetail {
          expected_operation_id: "current".into(),
          received_operation_id: "stale".into(),
        },
      )),
    ),
    (
      RuntimeCommandError::ModelDownloadCancellationTooLate,
      Code::FailedPrecondition,
      CommandErrorCode::ModelDownloadCancellationTooLate,
      None,
    ),
    (
      RuntimeCommandError::TurnAlreadyActive,
      Code::FailedPrecondition,
      CommandErrorCode::TurnAlreadyActive,
      None,
    ),
    (
      RuntimeCommandError::NoActiveTurn,
      Code::FailedPrecondition,
      CommandErrorCode::NoActiveTurn,
      None,
    ),
    (
      RuntimeCommandError::EmptyText,
      Code::InvalidArgument,
      CommandErrorCode::EmptyText,
      None,
    ),
    (
      RuntimeCommandError::Steering("wait for boundary".into()),
      Code::FailedPrecondition,
      CommandErrorCode::Steering,
      Some(Context::SteeringReason("wait for boundary".into())),
    ),
    (
      RuntimeCommandError::NoPendingApproval,
      Code::FailedPrecondition,
      CommandErrorCode::NoPendingApproval,
      None,
    ),
    (
      RuntimeCommandError::ApprovalRequestMismatch {
        expected: expected_id,
        received: received_id,
      },
      Code::FailedPrecondition,
      CommandErrorCode::ApprovalRequestMismatch,
      Some(Context::ApprovalRequestMismatch(
        ApprovalRequestMismatchDetail {
          expected_request_id: expected_id.to_string(),
          received_request_id: received_id.to_string(),
        },
      )),
    ),
    (
      RuntimeCommandError::Unsupported(AppUnsupportedCommand::ForceStop),
      Code::FailedPrecondition,
      CommandErrorCode::Unsupported,
      Some(Context::UnsupportedCommand(
        WireUnsupportedCommand::ForceStop as i32,
      )),
    ),
  ];

  for (index, (error, tonic_code, detail_code, context)) in cases.into_iter().enumerate() {
    let status = status_for(error, index).await;
    assert_eq!(status.code(), tonic_code);
    let detail = CommandErrorDetail::decode(status.details()).unwrap();
    assert_eq!(detail.code, detail_code as i32);
    assert_eq!(detail.context, context);
  }
}

#[tokio::test]
async fn every_unsupported_command_preserves_its_variant() {
  use agent_app::UnsupportedCommand as AppUnsupportedCommand;

  let cases = [
    (
      AppUnsupportedCommand::ForceStop,
      WireUnsupportedCommand::ForceStop,
    ),
    (
      AppUnsupportedCommand::RespondApproval,
      WireUnsupportedCommand::RespondApproval,
    ),
    (
      AppUnsupportedCommand::SelectSessionModel,
      WireUnsupportedCommand::SelectSessionModel,
    ),
  ];

  for (index, (runtime, wire)) in cases.into_iter().enumerate() {
    let status = status_for(RuntimeCommandError::Unsupported(runtime), index).await;
    let detail = CommandErrorDetail::decode(status.details()).unwrap();
    assert_eq!(
      detail.context,
      Some(Context::UnsupportedCommand(wire as i32))
    );
  }
}
