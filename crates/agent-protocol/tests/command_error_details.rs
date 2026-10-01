use agent_protocol::v1::command_error_detail::Context;
use agent_protocol::v1::{
  ApprovalRequestMismatchDetail, CommandErrorCode, CommandErrorDetail, UnsupportedCommand,
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
  ];

  for detail in details {
    let decoded = CommandErrorDetail::decode(detail.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, detail);
  }
}
