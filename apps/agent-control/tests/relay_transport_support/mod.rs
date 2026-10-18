use std::time::Duration;

use agent_distributed::{
  peers::PeerManager,
  relay_transport::{RelayHandle, RelayIncoming, RelayTransport},
};
use agent_storage::Store;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::support::{Fixture, node};

pub struct Tunnels {
  pub fixture: Fixture,
  pub a_manager: PeerManager,
  pub b_manager: PeerManager,
  pub a: RelayHandle,
  pub b: RelayHandle,
  pub incoming_a: RelayIncoming,
  pub incoming_b: RelayIncoming,
  stop: CancellationToken,
  tasks: Vec<tokio::task::JoinHandle<Result<()>>>,
}
impl Tunnels {
  pub async fn start() -> Result<Self> {
    Self::with_queue(64).await
  }
  pub async fn with_queue(queue: usize) -> Result<Self> {
    logging();
    let fixture = Fixture::new(queue).await?;
    let a_manager = manager(&fixture, &fixture.a, "a.sqlite3")?;
    let b_manager = manager(&fixture, &fixture.b, "b.sqlite3")?;
    for (manager, peer) in [(&a_manager, &fixture.b), (&b_manager, &fixture.a)] {
      manager.pair_local(peer.public.clone(), vec!["tasks.read".into()])?;
      manager.pair_local(fixture.control.public.clone(), Vec::new())?;
    }
    let stop = CancellationToken::new();
    let (a, incoming_a, a_runtime) = RelayTransport::new(
      a_manager.clone(),
      "control".into(),
      fixture.endpoint.clone(),
      ["node-b".into()].into(),
      stop.child_token(),
    )?;
    let (b, incoming_b, b_runtime) = RelayTransport::new(
      b_manager.clone(),
      "control".into(),
      fixture.endpoint.clone(),
      ["node-a".into()].into(),
      stop.child_token(),
    )?;
    let tasks = vec![tokio::spawn(a_runtime.run()), tokio::spawn(b_runtime.run())];
    connected(&[&a, &b]).await?;
    Ok(Self {
      fixture,
      a_manager,
      b_manager,
      a,
      b,
      incoming_a,
      incoming_b,
      stop,
      tasks,
    })
  }
  pub async fn close(self) -> Result<()> {
    self.stop.cancel();
    for task in self.tasks {
      tokio::time::timeout(Duration::from_secs(5), task).await???;
    }
    Ok(())
  }
}

pub fn manager(
  fixture: &Fixture,
  identity: &agent_distributed::identity::PeerIdentity,
  file: &str,
) -> Result<PeerManager> {
  let store = Store::open(fixture.directory.path().join(file))?;
  let mut info = node(identity);
  // No listener exists here. Successful PeerManager::connect must use the relay route.
  info.endpoint = "https://127.0.0.1:1".into();
  PeerManager::new(store, identity.clone(), info)
}
pub async fn connected(handles: &[&RelayHandle]) -> Result<()> {
  tokio::time::timeout(Duration::from_secs(5), async {
    while !handles.iter().all(|handle| handle.is_connected()) {
      tokio::task::yield_now().await;
    }
  })
  .await?;
  Ok(())
}

pub struct RawPeer {
  pub fixture: Fixture,
  pub a: RelayHandle,
  pub remote: agent_distributed::relay_client::RelayConnection,
  pub manager: PeerManager,
  _incoming: RelayIncoming,
  stop: CancellationToken,
  task: tokio::task::JoinHandle<Result<()>>,
}
impl RawPeer {
  pub async fn start() -> Result<Self> {
    logging();
    let fixture = Fixture::new(64).await?;
    let manager = manager(&fixture, &fixture.a, "manual-a.sqlite3")?;
    manager.pair_local(fixture.b.public.clone(), Vec::new())?;
    manager.pair_local(fixture.control.public.clone(), Vec::new())?;
    let remote = fixture.client(&fixture.b).await?.open(64).await?;
    let stop = CancellationToken::new();
    let (a, incoming, runtime) = RelayTransport::new(
      manager.clone(),
      "control".into(),
      fixture.endpoint.clone(),
      ["node-b".into()].into(),
      stop.child_token(),
    )?;
    let task = tokio::spawn(runtime.run());
    connected(&[&a]).await?;
    Ok(Self {
      fixture,
      a,
      remote,
      manager,
      _incoming: incoming,
      stop,
      task,
    })
  }

  pub async fn authenticate(
    &mut self,
  ) -> Result<(
    agent_distributed::relay_transport::RelayIo,
    agent_distributed::secure_link::SecureLink,
    String,
  )> {
    use agent_distributed::secure_link::{LinkBinding, LinkHandshake};
    let handle = self.a.clone();
    let opened = tokio::spawn(async move { handle.open("node-b").await });
    let hello = tokio::time::timeout(Duration::from_secs(3), async {
      loop {
        let frame = self
          .remote
          .receive()
          .await?
          .ok_or_else(|| anyhow::anyhow!("remote closed"))?;
        if frame.seq == u64::MAX {
          return Ok::<_, anyhow::Error>(frame);
        }
      }
    })
    .await??;
    let binding = LinkBinding {
      link_id: hello.link_id.clone(),
      initiator: "node-a".into(),
      responder: "node-b".into(),
    };
    let mut handshake =
      LinkHandshake::new(&self.fixture.b, &self.fixture.a.public, binding, false)?;
    handshake.read(&hello.ciphertext)?;
    self.remote.try_send(agent_protocol::v1::RelayEnvelope {
      link_id: hello.link_id.clone(),
      source_node_id: "node-b".into(),
      target_node_id: "node-a".into(),
      seq: u64::MAX,
      ciphertext: handshake.write()?,
    })?;
    let io = opened.await??;
    Ok((io, handshake.finish()?, hello.link_id))
  }

  pub async fn close(self) -> Result<()> {
    self.stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), self.task).await???;
    Ok(())
  }
}

fn logging() {
  static LOGS: std::sync::OnceLock<(tempfile::TempDir, agent_telemetry::LogGuard)> =
    std::sync::OnceLock::new();
  LOGS.get_or_init(|| {
    let data = tempfile::tempdir().expect("relay transport test log directory");
    let guard = agent_telemetry::init_with_options(
      &agent_telemetry::LogOptions {
        directory: data.path().join("logs"),
        retained_files: 2,
        stderr: true,
      },
      "relay-transport-test",
      "warn,agent_distributed::relay_transport=debug",
    )
    .expect("relay test logging");
    (data, guard)
  });
}
