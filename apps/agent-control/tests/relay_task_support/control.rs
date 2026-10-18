use agent_control::ControlPlane;
use agent_core::TaskEnvelope;
use agent_distributed::{identity::PeerIdentity, peers::PeerManager};
use agent_protocol::v1::control_service_server::ControlServiceServer;
use agent_storage::Store;
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;

pub struct Control {
  pub store: Store,
  pub manager: PeerManager,
  pub identity: PeerIdentity,
  pub endpoint: String,
  directory: tempfile::TempDir,
  address: SocketAddr,
  plane: Option<ControlPlane>,
  server: Option<JoinHandle<()>>,
}
impl Control {
  pub async fn new() -> Self {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("control.sqlite3")).unwrap();
    let identity = PeerIdentity::generate("relay-control").unwrap();
    let manager = PeerManager::new(
      store.clone(),
      identity.clone(),
      agent_protocol::v1::Node {
        id: identity.public.node_id.clone(),
        protocol_version: 1,
        protocol_min: 1,
        protocol_max: 1,
        ..Default::default()
      },
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    Self {
      store,
      manager,
      identity,
      endpoint: format!("https://{address}"),
      directory,
      address,
      plane: None,
      server: None,
    }
  }
  pub async fn reconnect(&mut self) {
    assert!(self.server.is_none());
    let listener = TcpListener::bind(self.address).await.unwrap();
    let plane = ControlPlane::new(
      self.manager.clone(),
      self.store.clone(),
      Duration::from_secs(30),
      64,
    )
    .unwrap();
    let service = plane.clone();
    let tls = self.manager.server_tls().unwrap();
    self.server = Some(tokio::spawn(async move {
      tonic::transport::Server::builder()
        .tls_config(tls)
        .unwrap()
        .add_service(ControlServiceServer::new(service))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .unwrap();
    }));
    self.plane = Some(plane);
  }
  pub async fn disconnect(&mut self) {
    if let Some(plane) = self.plane.take() {
      plane.shutdown();
    }
    if let Some(server) = self.server.take() {
      server.abort();
      let _ = server.await;
    }
  }
  pub fn assert_no_task_plaintext(&self, task: &TaskEnvelope) {
    assert!(self.store.tasks().unwrap().is_empty());
    assert!(self.store.sessions().unwrap().is_empty());
    for entry in std::fs::read_dir(self.directory.path()).unwrap() {
      let entry = entry.unwrap();
      if !entry.file_type().unwrap().is_file() {
        continue;
      }
      let bytes = std::fs::read(entry.path()).unwrap();
      for private in [
        &task.instructions,
        "master-private-relay-content",
        "private-worker-result-over-relay",
      ] {
        assert!(
          !bytes
            .windows(private.len())
            .any(|part| part == private.as_bytes()),
          "control persisted private task/session content"
        );
      }
    }
  }
}
impl Drop for Control {
  fn drop(&mut self) {
    if let Some(plane) = &self.plane {
      plane.shutdown();
    }
    if let Some(server) = &self.server {
      server.abort();
    }
  }
}
