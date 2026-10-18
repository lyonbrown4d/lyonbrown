use std::time::Duration;

use agent_distributed::peers::PeerManager;
use agent_protocol::v1::{self, control_service_server::ControlService};
use agent_storage::Store;
use anyhow::{Result, ensure};
use prost::Message;
use tonic::{Request, Response, Status};

use crate::relay::{RelayHub, RelayOutput};

#[derive(Clone)]
pub struct ControlPlane {
  pub(crate) peers: PeerManager,
  pub(crate) store: Store,
  pub(crate) hub: RelayHub,
  lease_ms: i64,
}

impl ControlPlane {
  pub fn shutdown(&self) {
    self.hub.shutdown();
  }
  pub fn new(peers: PeerManager, store: Store, lease: Duration, queue: usize) -> Result<Self> {
    ensure!(
      (1..=1024).contains(&queue),
      "relay queue must be between 1 and 1024"
    );
    ensure!(
      (1..=3600).contains(&lease.as_secs()),
      "directory lease must be 1..3600 seconds"
    );
    Ok(Self {
      peers,
      store,
      hub: RelayHub::new(queue),
      lease_ms: i64::try_from(lease.as_millis())?,
    })
  }

  pub(crate) fn allow(&self, from: &str, to: &str) -> Result<(), Status> {
    for id in [from, to] {
      let trust = self
        .peers
        .trust(id)
        .map_err(|_| Status::permission_denied("relay node is not paired or has been revoked"))?;
      if !trust.capabilities.iter().any(|cap| cap == "control.relay") {
        return Err(Status::permission_denied("node has no relay capability"));
      }
    }
    if !self
      .store
      .control_link_allowed(from, to)
      .map_err(internal)?
    {
      return Err(Status::permission_denied(
        "operator has not authorized this directional link",
      ));
    }
    Ok(())
  }
}

#[tonic::async_trait]
impl ControlService for ControlPlane {
  async fn register(
    &self,
    request: Request<v1::RegisterRequest>,
  ) -> Result<Response<v1::Node>, Status> {
    let trust = self.peers.authorize(&request, &["control.register"])?;
    let registration = request.into_inner();
    let node = registration
      .node
      .ok_or_else(|| Status::invalid_argument("node is required"))?;
    if node.id != trust.node_id
      || node.fingerprint != trust.fingerprint
      || registration.public_key != trust.noise_public_key
    {
      return Err(Status::permission_denied(
        "registration does not match the paired TLS and Noise identity",
      ));
    }
    if node.encoded_len() > 16_384
      || node.protocol_min > node.protocol_max
      || node.protocol_version < node.protocol_min
      || node.protocol_version > node.protocol_max
      || !(node.endpoint.is_empty() || node.endpoint.starts_with("https://"))
    {
      return Err(Status::invalid_argument(
        "invalid or oversized public node metadata",
      ));
    }
    let expires = agent_core::now_ms()
      .checked_add(self.lease_ms)
      .ok_or_else(|| Status::internal("directory time overflow"))?;
    self
      .store
      .control_register(&node.id, &hex::encode(node.encode_to_vec()), expires)
      .map_err(internal)?;
    tracing::info!(operation = "control.register", outcome = "persisted", node_id = %node.id, expires_at_ms = expires, "paired node directory lease updated");
    Ok(Response::new(node))
  }

  async fn resolve(&self, request: Request<v1::PeerRequest>) -> Result<Response<v1::Node>, Status> {
    let trust = self.peers.authorize(&request, &["control.resolve"])?;
    let target = request.into_inner().node_id;
    self.allow(&trust.node_id, &target)?;
    let record = self
      .store
      .control_resolve(&target, agent_core::now_ms())
      .map_err(internal)?
      .ok_or_else(|| Status::not_found("node registration has expired or does not exist"))?;
    let bytes = hex::decode(record).map_err(internal)?;
    let node = v1::Node::decode(bytes.as_slice()).map_err(internal)?;
    if self.peers.trust(&target).map_err(internal)?.fingerprint != node.fingerprint {
      return Err(Status::not_found("node must register its rotated identity"));
    }
    Ok(Response::new(node))
  }

  async fn pair(
    &self,
    _request: Request<v1::PairRequest>,
  ) -> Result<Response<v1::PeerTrust>, Status> {
    Err(Status::permission_denied(
      "pairing requires local operator verification of the complete identity binding",
    ))
  }

  type RelayStream = RelayOutput;
  async fn relay(
    &self,
    request: Request<tonic::Streaming<v1::RelayEnvelope>>,
  ) -> Result<Response<Self::RelayStream>, Status> {
    let trust = self.peers.authorize(&request, &["control.relay"])?;
    let output = self
      .hub
      .attach(self.clone(), trust.node_id, request.into_inner())
      .await?;
    Ok(Response::new(output))
  }
}

pub(crate) fn internal(_error: impl std::fmt::Display) -> Status {
  Status::internal("control persistence or synchronization failed")
}
