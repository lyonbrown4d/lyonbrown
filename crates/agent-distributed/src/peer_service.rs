use agent_protocol::v1::{self, peer_service_server::PeerService};
use tonic::{Request, Response, Status};

use crate::peers::PeerManager;

#[tonic::async_trait]
impl PeerService for PeerManager {
  async fn hello(&self, request: Request<v1::Node>) -> Result<Response<v1::Node>, Status> {
    let peer = self.authorize(&request, &[])?;
    if peer.node_id != request.get_ref().id || peer.fingerprint != request.get_ref().fingerprint {
      return Err(Status::permission_denied(
        "claimed node does not match the mutual TLS identity",
      ));
    }
    self
      .remember(request.into_inner())
      .map_err(|_| Status::internal("peer directory unavailable"))?;
    Ok(Response::new(self.local_node()))
  }
  async fn list_peers(&self, request: Request<v1::Empty>) -> Result<Response<v1::Nodes>, Status> {
    self.authorize(&request, &["peers.list"])?;
    Ok(Response::new(v1::Nodes {
      nodes: self
        .known_peers()
        .map_err(|_| Status::internal("peer directory unavailable"))?,
    }))
  }
  async fn pair(&self, _: Request<v1::PairRequest>) -> Result<Response<v1::PeerTrust>, Status> {
    Err(Status::permission_denied(
      "peer pairing requires a local human management action",
    ))
  }
  async fn revoke(&self, _: Request<v1::PeerRequest>) -> Result<Response<v1::PeerTrust>, Status> {
    Err(Status::permission_denied(
      "peer revocation requires a local human management action",
    ))
  }
}
