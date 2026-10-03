mod support;

use std::sync::Arc;

use agent_app::{RuntimeSnapshot, channel};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::model_selection::Selection;
use agent_protocol::v1::{
  CancelModelDownloadRequest, HandshakeRequest, HealthRequest, ModelRoute, ModelSelection,
  RequestMetadata, SelectSessionModelRequest, StartModelDownloadRequest, StartTurnRequest,
};
use agentd::AppControllerHost;

use support::{RawClient, TestServer, spawn_authenticated};

async fn service() -> (TestServer, RawClient) {
  let (controller, _runtime) = channel(RuntimeSnapshot::default(), 1);
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let server = spawn_authenticated(listener, Arc::new(AppControllerHost::new(controller)));
  let client = server.raw_client().await;
  (server, client)
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
  let (server, mut client) = service().await;
  let error = client
    .start_turn(StartTurnRequest {
      metadata: Some(metadata_for(&server, "client", "command")),
      text: "x".repeat(1024 * 1024 + 1),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("text exceeds"));
}

#[tokio::test]
async fn oversized_metadata_identifier_is_rejected() {
  let (_server, mut client) = service().await;
  let error = client
    .health(HealthRequest {
      metadata: Some(metadata("x".repeat(129), "command".into())),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("client_instance_id exceeds"));
}

#[tokio::test]
async fn oversized_handshake_name_is_rejected() {
  let (_server, mut client) = service().await;
  let error = client
    .handshake(HandshakeRequest {
      metadata: Some(metadata("client".into(), "handshake".into())),
      client_name: "x".repeat(129),
      client_version: "1.0".into(),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("client_name exceeds"));
}

#[tokio::test]
async fn oversized_pinned_model_name_is_rejected() {
  let (server, mut client) = service().await;
  let error = client
    .select_session_model(SelectSessionModelRequest {
      metadata: Some(metadata_for(&server, "client", "select")),
      selection: Some(ModelSelection {
        selection: Some(Selection::Pinned(ModelRoute {
          provider: "local".into(),
          model: "x".repeat(257),
        })),
      }),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("model exceeds"));
}

#[tokio::test]
async fn pinned_route_at_shared_limits_passes_rpc_validation() {
  let (server, mut client) = service().await;
  let result = client
    .select_session_model(SelectSessionModelRequest {
      metadata: Some(metadata_for(&server, "client", "select-at-limit")),
      selection: Some(ModelSelection {
        selection: Some(Selection::Pinned(ModelRoute {
          provider: "p".repeat(128),
          model: "m".repeat(256),
        })),
      }),
    })
    .await;

  if let Err(error) = result {
    assert_ne!(error.code(), tonic::Code::InvalidArgument);
  }
}

#[tokio::test]
async fn model_download_identifiers_are_required_and_bounded() {
  let (server, mut client) = service().await;
  let error = client
    .start_model_download(StartModelDownloadRequest {
      metadata: Some(metadata_for(&server, "client", "download")),
      model_id: "x".repeat(129),
    })
    .await
    .unwrap_err();
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("model_id exceeds"));

  let error = client
    .cancel_model_download(CancelModelDownloadRequest {
      metadata: Some(metadata_for(&server, "client", "cancel-download")),
      operation_id: " ".into(),
    })
    .await
    .unwrap_err();
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("operation_id is required"));
}

fn metadata_for(server: &TestServer, client: &str, command: &str) -> RequestMetadata {
  RequestMetadata {
    expected_server_instance_id: server.server_instance_id(),
    ..metadata(client.into(), command.into())
  }
}
