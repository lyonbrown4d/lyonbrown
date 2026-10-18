//! Opt-in peer transport; discovery never changes the local pairing authority.
mod executor;
mod maintenance;
mod service;

use std::sync::Arc;

use agent_distributed::{
  discovery::Discovery,
  identity::PeerIdentity,
  peers::PeerManager,
  relay_transport::{PeerIo, RelayIncoming, RelayRuntime, RelayTransport},
  worker::TaskWorker,
};
use agent_protocol::v1::{
  self, peer_service_server::PeerServiceServer, task_service_server::TaskServiceServer,
};
use anyhow::Result;

use crate::Daemon;
pub use maintenance::{Maintenance, maintain};

/// Uses only this daemon's durable identity and explicitly configured listener origin.
pub fn manager(daemon: &Daemon) -> Result<PeerManager> {
  let identity =
    PeerIdentity::load_or_create(&daemon.config.data_path()?.join("peers"), &daemon.node_id)?;
  let manager = PeerManager::new(
    daemon.store.clone(),
    identity,
    v1::Node {
      id: daemon.node_id.clone(),
      name: "Lyonbrown".into(),
      os: std::env::consts::OS.into(),
      endpoint: daemon
        .config
        .peer
        .as_ref()
        .map(|peer| peer.advertise_url.clone())
        .unwrap_or_default(),
      fingerprint: String::new(),
      capabilities: vec!["workspace.read".into(), "workspace.write.approval".into()],
      protocol_version: 1,
      daemon_version: env!("CARGO_PKG_VERSION").into(),
      protocol_min: 1,
      protocol_max: 1,
      active_runs: 0,
      storage_schema_version: 1,
      draining: false,
    },
  )?;
  Ok(match daemon.relay.get() {
    Some(relay) => manager.with_relay(relay.clone()),
    None => manager,
  })
}

pub struct PeerRuntime {
  daemon: Daemon,
  manager: PeerManager,
  worker: Arc<TaskWorker>,
  tls: tonic::transport::ServerTlsConfig,
  settings: agent_config::PeerSettings,
  listener: tokio::net::TcpListener,
  relay: Option<(RelayIncoming, RelayRuntime)>,
}

impl PeerRuntime {
  /// Recover attempts and freeze the CA set before accepting any remote request.
  pub async fn prepare(daemon: Daemon) -> Result<Option<Self>> {
    let Some(settings) = daemon.config.peer.clone() else {
      return Ok(None);
    };
    settings.validate()?;
    // Reserve the peer endpoint before reporting readiness or installing Worker state.
    let listener = tokio::net::TcpListener::bind(settings.listen).await?;
    let mut installed = daemon.peer_worker.lock().await;
    anyhow::ensure!(installed.is_none(), "peer runtime is already installed");
    let mut manager = manager(&daemon)?;
    let relay = if let Some(settings) = &daemon.config.relay {
      settings.validate()?;
      let (handle, incoming, runtime) = RelayTransport::new(
        manager.clone(),
        settings.control_node_id.clone(),
        settings.endpoint.clone(),
        settings.routes.clone(),
        daemon.stopping.child_token(),
      )?;
      daemon
        .relay
        .set(handle.clone())
        .map_err(|_| anyhow::anyhow!("relay runtime is already installed"))?;
      manager = manager.with_relay(handle);
      Some((incoming, runtime))
    } else {
      None
    };
    let tls = manager.server_tls()?;
    let worker = Arc::new(TaskWorker::new(
      manager.clone(),
      Arc::new(executor::HarnessWorker(daemon.clone())),
      daemon.config.data_path()?.join("worker-workspaces"),
    )?);
    let recovered = worker.recover_uncertain()?;
    tracing::info!(
      recovered,
      "reconciled interrupted worker attempts without replay"
    );
    *installed = Some(worker.control());
    drop(installed);
    Ok(Some(Self {
      daemon,
      manager,
      worker,
      tls,
      settings,
      listener,
      relay,
    }))
  }

  pub async fn serve(self) -> Result<()> {
    use std::{io, pin::Pin};
    use tokio_stream::{Stream, StreamExt, wrappers::TcpListenerStream};

    let tcp = TcpListenerStream::new(self.listener).map(|value| value.map(PeerIo::Tcp));
    type Incoming = Pin<Box<dyn Stream<Item = io::Result<PeerIo>> + Send>>;
    let (incoming, relay): (Incoming, Option<RelayRuntime>) = match self.relay {
      Some((incoming, runtime)) => (
        Box::pin(tcp.merge(incoming.map(|value| value.map(PeerIo::Relay)))),
        Some(runtime),
      ),
      None => (Box::pin(tcp), None),
    };
    let discovery = if self.settings.mdns {
      Some(Discovery::advertise(
        &self.daemon.node_id,
        1,
        self.settings.listen,
      )?)
    } else {
      None
    };
    let daemon = self.daemon.clone();
    let worker = self.worker;
    let service = service::WorkerService {
      daemon: daemon.clone(),
      worker: worker.clone(),
    };
    let server = tonic::transport::Server::builder()
      .tls_config(self.tls)?
      .add_service(PeerServiceServer::new(self.manager))
      .add_service(TaskServiceServer::new(service))
      .serve_with_incoming_shutdown(incoming, daemon.stopping.cancelled());
    let discover = async {
      if let Some(discovery) = discovery {
        loop {
          // Candidates have no authentication or routing authority. Operators verify
          // identities out of band before pairing or dispatching to an endpoint.
          let _candidate = discovery.next().await?;
        }
      }
      std::future::pending::<Result<()>>().await
    };
    let result = tokio::select! {
      result = server => result.map_err(Into::into),
      result = discover => result,
      result = async {
        match relay {
          Some(runtime) => runtime.run().await,
          None => std::future::pending::<Result<()>>().await,
        }
      } => result,
    };
    // Keep the lifecycle handle alive until every accepted task has received
    // cancellation, including when the listener itself fails.
    daemon.stopping.cancel();
    worker.shutdown().await;
    result
  }
}
