use std::sync::Arc;

use agent_app::{RuntimeSnapshot, channel};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::agent_service_server::AgentService;
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{
  HandshakeRequest, HealthRequest, ModelRoute, ModelSelection, RequestMetadata,
  SelectSessionModelRequest, StartTurnRequest,
};
use agentd::{AgentGrpcService, AppControllerHost};
use tonic::Request;

fn service() -> AgentGrpcService {
  let (controller, _runtime) = channel(RuntimeSnapshot::default(), 1);
  AgentGrpcService::new(Arc::new(AppControllerHost::new(controller)))
}

fn metadata(client_instance_id: String, command_id: String) -> RequestMetadata {
  RequestMetadata {
    protocol_version: PROTOCOL_VERSION,
    client_instance_id,
    command_id,
    expected_server_instance_id: String::new(),
  }
}

#[tokio::test]
async fn oversized_command_text_is_rejected_before_runtime_execution() {
  let error = service()
    .start_turn(Request::new(StartTurnRequest {
      metadata: Some(metadata("client".into(), "command".into())),
      text: "x".repeat(1024 * 1024 + 1),
    }))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("text exceeds"));
}

#[tokio::test]
async fn oversized_metadata_identifier_is_rejected() {
  let error = service()
    .health(Request::new(HealthRequest {
      metadata: Some(metadata("x".repeat(129), "command".into())),
    }))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("client_instance_id exceeds"));
}

#[tokio::test]
async fn oversized_handshake_name_is_rejected() {
  let error = service()
    .handshake(Request::new(HandshakeRequest {
      metadata: Some(metadata("client".into(), "handshake".into())),
      client_name: "x".repeat(129),
      client_version: "1.0".into(),
    }))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("client_name exceeds"));
}

#[tokio::test]
async fn oversized_pinned_model_name_is_rejected() {
  let error = service()
    .select_session_model(Request::new(SelectSessionModelRequest {
      metadata: Some(metadata("client".into(), "select".into())),
      selection: Some(ModelSelection {
        selection: Some(Selection::Pinned(ModelRoute {
          provider: "local".into(),
          model: "x".repeat(257),
        })),
      }),
    }))
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("model exceeds"));
}
