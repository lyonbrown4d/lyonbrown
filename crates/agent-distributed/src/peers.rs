use std::{
  collections::HashMap,
  sync::{Arc, RwLock},
  time::Duration,
};

use agent_core::PeerTrust;
use agent_protocol::v1::{
  self, peer_service_client::PeerServiceClient, task_service_client::TaskServiceClient,
};
use agent_storage::Store;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use tonic::{
  Request, Status,
  transport::{
    Channel, Endpoint, ServerTlsConfig,
    server::{TcpConnectInfo, TlsConnectInfo},
  },
};

use crate::identity::{PeerIdentity, PublicIdentity};

#[derive(Clone)]
pub struct PeerManager {
  pub(crate) store: Store,
  pub(crate) identity: Arc<PeerIdentity>,
  pub(crate) node: v1::Node,
  known_nodes: Arc<RwLock<HashMap<String, v1::Node>>>,
  relay: Option<crate::relay_transport::RelayHandle>,
}

impl PeerManager {
  pub fn new(store: Store, identity: PeerIdentity, mut node: v1::Node) -> Result<Self> {
    ensure!(
      node.id == identity.public.node_id,
      "node and cryptographic identity differ"
    );
    node.fingerprint = identity.public.fingerprint()?;
    Ok(Self {
      store,
      identity: Arc::new(identity),
      node,
      known_nodes: Arc::default(),
      relay: None,
    })
  }
  pub fn local_node(&self) -> v1::Node {
    self.node.clone()
  }
  /// Routing selects a configured transport before connecting; failures never fall through.
  pub fn with_relay(mut self, relay: crate::relay_transport::RelayHandle) -> Self {
    self.relay = Some(relay);
    self
  }
  pub fn public_identity(&self) -> PublicIdentity {
    self.identity.public.clone()
  }

  pub(crate) fn remember(&self, node: v1::Node) -> Result<()> {
    self
      .known_nodes
      .write()
      .map_err(|_| anyhow::anyhow!("peer directory lock is unavailable"))?
      .insert(node.id.clone(), node);
    Ok(())
  }

  pub(crate) fn known_peers(&self) -> Result<Vec<v1::Node>> {
    let mut nodes = self
      .known_nodes
      .read()
      .map_err(|_| anyhow::anyhow!("peer directory lock is unavailable"))?
      .values()
      .filter(|node| self.trust(&node.id).is_ok())
      .cloned()
      .collect::<Vec<_>>();
    nodes.sort_by(|first, second| first.id.cmp(&second.id));
    Ok(nodes)
  }

  /// Call only from an explicitly authorized local human management path.
  /// An updated CA set requires reloading the peer listener.
  pub fn pair_local(
    &self,
    identity: PublicIdentity,
    capabilities: Vec<String>,
  ) -> Result<PeerTrust> {
    identity.validate()?;
    ensure!(
      identity.node_id != self.node.id,
      "cannot pair the local node as its own peer"
    );
    ensure!(
      capabilities.len() <= 128
        && capabilities
          .iter()
          .all(|cap| !cap.is_empty() && cap.len() <= 128),
      "invalid peer capabilities"
    );
    let fingerprint = identity.fingerprint()?;
    ensure!(
      !self
        .store
        .peers()?
        .iter()
        .any(|peer| peer.node_id != identity.node_id && peer.fingerprint == fingerprint),
      "certificate is already bound to another node identity"
    );
    let trust = PeerTrust {
      node_id: identity.node_id,
      fingerprint,
      capabilities,
      revoked: false,
      certificate_pem: identity.certificate_pem,
      noise_public_key: identity.noise_public_key,
    };
    self.store.trust_peer(&trust)?;
    tracing::info!(
      operation = "peer.pair",
      outcome = "persisted",
      "local peer trust updated; reload the peer listener CA set"
    );
    Ok(trust)
  }

  pub fn revoke_local(&self, id: &str) -> Result<PeerTrust> {
    let mut trust = self.store.peer(id)?.context("unknown peer")?;
    trust.revoked = true;
    self.store.trust_peer(&trust)?;
    tracing::info!(
      operation = "peer.revoke",
      outcome = "persisted",
      "peer requests and streams will be denied"
    );
    Ok(trust)
  }

  pub fn trust(&self, id: &str) -> Result<PeerTrust> {
    let trust = self.store.peer(id)?.context("peer has not been paired")?;
    ensure!(!trust.revoked, "peer trust is revoked");
    ensure!(
      public_identity(&trust).fingerprint()? == trust.fingerprint,
      "stored peer certificate fingerprint differs"
    );
    Ok(trust)
  }

  pub fn server_tls(&self) -> Result<ServerTlsConfig> {
    let peers = self
      .store
      .peers()?
      .into_iter()
      .filter(|peer| !peer.revoked)
      .map(|peer| public_identity(&peer));
    self.identity.server_tls(peers)
  }

  pub fn authorize<T>(
    &self,
    request: &Request<T>,
    capabilities: &[&str],
  ) -> Result<PeerTrust, Status> {
    let fingerprint = request
      .extensions()
      .get::<TlsConnectInfo<TcpConnectInfo>>()
      .and_then(|info| info.peer_certs())
      .and_then(|certs| {
        certs
          .first()
          .map(|cert| hex::encode(Sha256::digest(cert.as_ref())))
      })
      .ok_or_else(|| Status::unauthenticated("a paired mutual TLS client identity is required"))?;
    let trust = self
      .store
      .peers()
      .map_err(|_| Status::internal("peer trust store unavailable"))?
      .into_iter()
      .find(|peer| peer.fingerprint == fingerprint && !peer.revoked)
      .ok_or_else(|| Status::permission_denied("client certificate is not an authorized peer"))?;
    require_capabilities(&trust, capabilities)?;
    Ok(trust)
  }

  pub async fn connect(&self, remote: &v1::Node) -> Result<PeerConnection> {
    let trust = self.trust(&remote.id)?;
    ensure!(
      remote.endpoint.starts_with("https://"),
      "peer endpoints require HTTPS"
    );
    let endpoint = Endpoint::from_shared(remote.endpoint.clone())?
      .connect_timeout(Duration::from_secs(3))
      .timeout(Duration::from_secs(5))
      .tls_config(self.identity.client_tls(&public_identity(&trust))?)?;
    let channel = if let Some(relay) = &self.relay
      && relay.routes(&remote.id)
    {
      relay
        .channel(&remote.id, endpoint)
        .await
        .context("connect to paired peer over relayed mutual TLS")?
    } else {
      endpoint
        .connect()
        .await
        .context("connect to paired peer over mutual TLS")?
    };
    let mut peer = PeerServiceClient::new(channel.clone());
    let hello = peer.hello(self.node.clone()).await?.into_inner();
    ensure!(
      hello.id == trust.node_id && hello.fingerprint == trust.fingerprint,
      "peer handshake does not match its paired identity"
    );
    ensure!(
      hello.protocol_min <= self.node.protocol_version
        && self.node.protocol_version <= hello.protocol_max,
      "peer protocol is incompatible"
    );
    self.remember(hello.clone())?;
    Ok(PeerConnection {
      node: hello,
      tasks: TaskServiceClient::new(channel),
    })
  }
}

#[derive(Clone)]
pub struct PeerConnection {
  pub node: v1::Node,
  pub(crate) tasks: TaskServiceClient<Channel>,
}

pub(crate) fn public_identity(trust: &PeerTrust) -> PublicIdentity {
  PublicIdentity {
    node_id: trust.node_id.clone(),
    certificate_pem: trust.certificate_pem.clone(),
    noise_public_key: trust.noise_public_key.clone(),
  }
}

pub(crate) fn require_capabilities(trust: &PeerTrust, capabilities: &[&str]) -> Result<(), Status> {
  if capabilities
    .iter()
    .all(|required| trust.capabilities.iter().any(|cap| cap == required))
  {
    Ok(())
  } else {
    Err(Status::permission_denied(
      "peer capability scope is insufficient",
    ))
  }
}
