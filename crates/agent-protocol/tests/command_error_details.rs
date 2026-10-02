use agent_protocol::v1::command_error_detail::Context;
use agent_protocol::v1::{
  ApprovalRequestMismatchDetail, CommandErrorCode, CommandErrorDetail, EpochMismatchDetail,
  InvalidModelIdDetail, ModelDownloadOperationMismatchDetail, UnsupportedCommand,
};
use prost::Message;

#[test]
fn command_error_details_round_trip_every_context_shape() {
  let details = [
    CommandErrorDetail {
      code: CommandErrorCode::Steering as i32,
      context: Some(Context::SteeringReason("busy".into())),
    },
    CommandErrorDetail {
      code: CommandErrorCode::ApprovalRequestMismatch as i32,
      context: Some(Context::ApprovalRequestMismatch(
        ApprovalRequestMismatchDetail {
          expected_request_id: "expected".into(),
          received_request_id: "received".into(),
        },
      )),
    },
    CommandErrorDetail {
      code: CommandErrorCode::Unsupported as i32,
      context: Some(Context::UnsupportedCommand(
        UnsupportedCommand::ForceStop as i32,
      )),
    },
    CommandErrorDetail {
      code: CommandErrorCode::InvalidModelId as i32,
      context: Some(Context::InvalidModelId(InvalidModelIdDetail {
        requested_model_id: "other".into(),
        configured_model_id: "configured".into(),
      })),
    },
    CommandErrorDetail {
      code: CommandErrorCode::ModelDownloadOperationMismatch as i32,
      context: Some(Context::ModelDownloadOperationMismatch(
        ModelDownloadOperationMismatchDetail {
          expected_operation_id: "current".into(),
          received_operation_id: "stale".into(),
        },
      )),
    },
    CommandErrorDetail {
      code: CommandErrorCode::EpochMismatch as i32,
      context: Some(Context::EpochMismatch(EpochMismatchDetail {
        expected_server_instance_id: "daemon-a".into(),
        actual_server_instance_id: "daemon-b".into(),
      })),
    },
  ];

  for detail in details {
    let decoded = CommandErrorDetail::decode(detail.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, detail);
  }
}
