//! Outbound-only mTLS control connection. Reconnect never replays application frames.
use std::time::Duration;

use agent_protocol::v1::{self, control_service_client::ControlServiceClient};
use anyhow::{Context, Result, ensure};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
  Status,
  transport::{Channel, Endpoint},
};

use crate::identity::{PeerIdentity, PublicIdentity};

#[derive(Clone)]
pub struct RelayClient {
  client: ControlServiceClient<Channel>,
  node_id: String,
  public_key: String,
  fingerprint: String,
}

impl RelayClient {
  /// Both control identity and remote Noise peers must be pinned by local pairing.
  pub async fn connect(
    endpoint: &str,
    identity: &PeerIdentity,
    control: &PublicIdentity,
  ) -> Result<Self> {
    ensure!(endpoint.starts_with("https://"), "control requires HTTPS");
    let channel = Endpoint::from_shared(endpoint.to_owned())?
      .connect_timeout(Duration::from_secs(5))
      .tls_config(identity.client_tls(control)?)?
      .connect()
      .await
      .context("connect to pinned control with mutual TLS")?;
    Ok(Self {
      client: ControlServiceClient::new(channel),
      node_id: identity.public.node_id.clone(),
      public_key: identity.public.noise_public_key.clone(),
      fingerprint: identity.public.fingerprint()?,
    })
  }

  pub async fn register(&mut self, mut node: v1::Node) -> Result<v1::Node> {
    ensure!(node.id == self.node_id, "cannot register a different node");
    node.fingerprint = self.fingerprint.clone();
    Ok(
      tokio::time::timeout(
        Duration::from_secs(5),
        self.client.register(v1::RegisterRequest {
          node: Some(node),
          public_key: self.public_key.clone(),
        }),
      )
      .await
      .context("control registration timed out; outcome may be unknown")??
      .into_inner(),
    )
  }

  pub async fn resolve(&mut self, node: &str) -> Result<v1::Node> {
    Ok(
      tokio::time::timeout(
        Duration::from_secs(5),
        self.client.resolve(v1::PeerRequest {
          node_id: node.into(),
        }),
      )
      .await
      .context("control lookup timed out")??
      .into_inner(),
    )
  }

  pub async fn open(&mut self, queue: usize) -> Result<RelayConnection> {
    ensure!((1..=1024).contains(&queue), "relay queue must be 1..1024");
    let (sender, receiver) = mpsc::channel(queue);
    let incoming = tokio::time::timeout(
      Duration::from_secs(5),
      self.client.relay(ReceiverStream::new(receiver)),
    )
    .await
    .context("open outbound relay timed out")??
    .into_inner();
    Ok(RelayConnection {
      sender,
      incoming,
      node_id: self.node_id.clone(),
    })
  }
}

pub struct RelayConnection {
  sender: mpsc::Sender<v1::RelayEnvelope>,
  incoming: tonic::Streaming<v1::RelayEnvelope>,
  node_id: String,
}
impl RelayConnection {
  pub(crate) fn into_parts(self) -> (RelaySender, tonic::Streaming<v1::RelayEnvelope>) {
    (
      RelaySender {
        sender: self.sender,
        node_id: self.node_id,
      },
      self.incoming,
    )
  }
  /// Enqueue once. Success is local acceptance, never confirmation of remote execution.
  pub fn try_send(&self, frame: v1::RelayEnvelope) -> Result<(), Status> {
    if frame.source_node_id != self.node_id {
      return Err(Status::permission_denied(
        "source identity differs from outbound connection",
      ));
    }
    self.sender.try_send(frame).map_err(|error| match error {
      mpsc::error::TrySendError::Full(_) => {
        Status::resource_exhausted("outbound relay queue is full")
      }
      mpsc::error::TrySendError::Closed(_) => {
        Status::unavailable("outbound relay connection closed; delivery result may be unknown")
      }
    })
  }

  pub async fn receive(&mut self) -> Result<Option<v1::RelayEnvelope>, Status> {
    self.incoming.message().await
  }
}

pub(crate) struct RelaySender {
  sender: mpsc::Sender<v1::RelayEnvelope>,
  node_id: String,
}
impl RelaySender {
  /// Wait for bounded queue space, enqueue once, and never replay across generations.
  pub async fn send(&self, frame: v1::RelayEnvelope) -> Result<(), Status> {
    if frame.source_node_id != self.node_id {
      return Err(Status::permission_denied(
        "source identity differs from outbound connection",
      ));
    }
    self.sender.send(frame).await.map_err(|_| {
      Status::unavailable("outbound relay connection closed; delivery result may be unknown")
    })
  }
}
