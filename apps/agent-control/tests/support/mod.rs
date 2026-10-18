#![allow(dead_code)]
use std::time::Duration;

use agent_control::ControlPlane;
use agent_distributed::{identity::PeerIdentity, peers::PeerManager, relay_client::RelayClient};
use agent_protocol::v1::{
  self, control_service_client::ControlServiceClient, control_service_server::ControlServiceServer,
};
use agent_storage::Store;
use anyhow::Result;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Endpoint};

pub struct Fixture {
  pub directory: tempfile::TempDir,
  pub control: PeerIdentity,
  pub a: PeerIdentity,
  pub b: PeerIdentity,
  pub manager: PeerManager,
  pub plane: ControlPlane,
  pub store: Store,
  pub endpoint: String,
  server: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl Fixture {
  pub async fn new(queue: usize) -> Result<Self> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path().join("control.sqlite3"))?;
    let control = PeerIdentity::generate("control")?;
    let a = PeerIdentity::generate("node-a")?;
    let b = PeerIdentity::generate("node-b")?;
    let manager = PeerManager::new(store.clone(), control.clone(), node(&control))?;
    for identity in [&a, &b] {
      manager.pair_local(
        identity.public.clone(),
        ["control.register", "control.resolve", "control.relay"]
          .map(String::from)
          .to_vec(),
      )?;
    }
    store.control_allow_link("node-a", "node-b", true)?;
    store.control_allow_link("node-b", "node-a", true)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("https://{}", listener.local_addr()?);
    let plane = ControlPlane::new(
      manager.clone(),
      store.clone(),
      Duration::from_secs(1),
      queue,
    )?;
    let mut server = tonic::transport::Server::builder().tls_config(manager.server_tls()?)?;
    let server_plane = plane.clone();
    let task = tokio::spawn(async move {
      server
        .add_service(ControlServiceServer::new(server_plane))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
    });
    Ok(Self {
      directory,
      store,
      control,
      a,
      b,
      manager,
      plane,
      endpoint,
      server: task,
    })
  }
  pub async fn raw(&self, identity: &PeerIdentity) -> Result<ControlServiceClient<Channel>> {
    let channel = Endpoint::from_shared(self.endpoint.clone())?
      .connect_timeout(Duration::from_secs(2))
      .tls_config(identity.client_tls(&self.control.public)?)?
      .connect()
      .await?;
    Ok(ControlServiceClient::new(channel))
  }
  pub async fn client(&self, identity: &PeerIdentity) -> Result<RelayClient> {
    RelayClient::connect(&self.endpoint, identity, &self.control.public).await
  }
}
impl Drop for Fixture {
  fn drop(&mut self) {
    self.server.abort();
  }
}
pub fn node(identity: &PeerIdentity) -> v1::Node {
  v1::Node {
    id: identity.public.node_id.clone(),
    fingerprint: identity.public.fingerprint().expect("test certificate"),
    protocol_version: 1,
    protocol_min: 1,
    protocol_max: 1,
    ..Default::default()
  }
}
pub fn frame(source: &str, target: &str, ciphertext: Vec<u8>) -> v1::RelayEnvelope {
  v1::RelayEnvelope {
    link_id: "test-link".into(),
    source_node_id: source.into(),
    target_node_id: target.into(),
    seq: 0,
    ciphertext,
  }
}
