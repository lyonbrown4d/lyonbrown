use memory_core::{EmbeddingDescriptor, IndexError};
use memory_qdrant::QdrantRemoteIndex;
use qdrant_client::qdrant::{
  HealthCheckReply, HealthCheckRequest,
  qdrant_server::{Qdrant, QdrantServer},
};
use tokio_stream::wrappers::TcpListenerStream;

struct IncompatibleServer;
#[tonic::async_trait]
impl Qdrant for IncompatibleServer {
  async fn health_check(
    &self,
    _: tonic::Request<HealthCheckRequest>,
  ) -> Result<tonic::Response<HealthCheckReply>, tonic::Status> {
    Ok(tonic::Response::new(HealthCheckReply {
      title: "synthetic protocol fixture".into(),
      version: "0.1.0".into(),
      commit: None,
    }))
  }
}

#[tokio::test]
async fn remote_rejects_incompatible_server_before_collection_access()
-> Result<(), Box<dyn std::error::Error>> {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let endpoint = format!("http://{}", listener.local_addr()?);
  let (stop, stopped) = tokio::sync::oneshot::channel();
  let server = tokio::spawn(
    tonic::transport::Server::builder()
      .add_service(QdrantServer::new(IncompatibleServer))
      .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
        let _ = stopped.await;
      }),
  );
  agent_security::initialize_tls();
  let model = rig_core::providers::openai::OpenAIConfig::new("synthetic-local-key")
    .with_base_url(&endpoint)
    .connect(rig_reqwest::ReqwestClient::default())
    .embedding("synthetic", Some(3))
    .erase();
  let result = QdrantRemoteIndex::connect(
    &endpoint,
    None,
    "fixture",
    EmbeddingDescriptor {
      id: "fixture".into(),
      model: "synthetic".into(),
      dimensions: 3,
      configuration_revision: "v1".into(),
    },
    model,
  )
  .await;
  assert!(matches!(result, Err(IndexError::Unsupported)));
  let _ = stop.send(());
  server.await??;
  Ok(())
}
