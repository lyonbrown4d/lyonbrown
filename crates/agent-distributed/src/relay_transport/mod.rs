//! Bounded Noise byte tunnels; ordinary tonic mutual TLS still owns task authorization.
mod bridge;
mod io;
mod session;

use std::{
  collections::BTreeSet,
  sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
  },
  time::Duration,
};

use anyhow::{Context, Result, ensure};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::{Channel, Endpoint};

use crate::{
  peers::{PeerManager, public_identity},
  relay_client::RelayClient,
};
pub use io::{PeerIo, RelayIo};

pub type RelayIncoming = ReceiverStream<std::io::Result<RelayIo>>;
const QUEUE: usize = 64;
const CONNECT: Duration = Duration::from_secs(3);

pub(super) enum Command {
  Open {
    peer: String,
    generation: u64,
    reply: oneshot::Sender<Result<RelayIo>>,
  },
  Data {
    id: String,
    generation: u64,
    bytes: Vec<u8>,
  },
  Fin {
    id: String,
    generation: u64,
  },
  Credit {
    id: String,
    generation: u64,
  },
  Drained {
    id: String,
    generation: u64,
  },
  Abort {
    id: String,
    generation: u64,
  },
}

#[derive(Clone)]
pub struct RelayHandle {
  commands: mpsc::Sender<Command>,
  generation: Arc<AtomicU64>,
  routes: Arc<BTreeSet<String>>,
}
impl RelayHandle {
  pub fn routes(&self, peer: &str) -> bool {
    self.routes.contains(peer)
  }
  pub fn is_connected(&self) -> bool {
    self.generation.load(Ordering::Acquire) != 0
  }

  pub async fn open(&self, peer: &str) -> Result<RelayIo> {
    ensure!(
      self.routes(peer),
      "peer is not configured for relay transport"
    );
    let generation = self.generation.load(Ordering::Acquire);
    ensure!(
      generation != 0,
      "relay is disconnected; no direct-route fallback is permitted"
    );
    let (reply, received) = oneshot::channel();
    self
      .commands
      .try_send(Command::Open {
        peer: peer.into(),
        generation,
        reply,
      })
      .map_err(|_| anyhow::anyhow!("relay command queue is full or closed"))?;
    tokio::time::timeout(CONNECT, received)
      .await
      .context("relay peer authentication timed out")?
      .context("relay generation closed during peer authentication")?
  }

  pub(crate) async fn channel(&self, peer: &str, endpoint: Endpoint) -> Result<Channel> {
    let stream = Arc::new(Mutex::new(Some(self.open(peer).await?)));
    let connector = tower::service_fn(move |_| {
      let stream = stream.clone();
      async move {
        stream
          .lock()
          .await
          .take()
          .map(hyper_util::rt::TokioIo::new)
          .ok_or_else(|| {
            std::io::Error::new(
              std::io::ErrorKind::ConnectionAborted,
              "relay tunnel is one-shot; reconcile before opening another connection",
            )
          })
      }
    });
    Ok(endpoint.connect_with_connector(connector).await?)
  }
}

pub struct RelayTransport;
pub struct RelayRuntime {
  manager: PeerManager,
  control: String,
  endpoint: String,
  commands: mpsc::Receiver<Command>,
  handle: RelayHandle,
  incoming: mpsc::Sender<std::io::Result<RelayIo>>,
  stop: CancellationToken,
  recent: session::Recent,
}
impl RelayTransport {
  #[allow(
    clippy::new_ret_no_self,
    reason = "factory separates connector, incoming stream, and owned lifecycle ports"
  )]
  pub fn new(
    manager: PeerManager,
    control: String,
    endpoint: String,
    routes: BTreeSet<String>,
    stop: CancellationToken,
  ) -> Result<(RelayHandle, RelayIncoming, RelayRuntime)> {
    ensure!(
      !routes.is_empty()
        && routes.len() <= 32
        && !routes.contains(&manager.node.id)
        && !routes.contains(&control)
        && routes.iter().all(|id| !id.is_empty() && id.len() <= 128),
      "invalid relay routes"
    );
    let origin = url_origin(&endpoint)?;
    ensure!(
      control != manager.node.id,
      "relay control cannot be the local peer"
    );
    manager.trust(&control)?;
    let (commands, receiver) = mpsc::channel(QUEUE);
    let (incoming, accepted) = mpsc::channel(8);
    let handle = RelayHandle {
      commands,
      generation: Arc::new(AtomicU64::new(0)),
      routes: Arc::new(routes),
    };
    let runtime = RelayRuntime {
      manager,
      control,
      endpoint: origin,
      commands: receiver,
      handle: handle.clone(),
      incoming,
      stop: stop.child_token(),
      recent: session::Recent::default(),
    };
    Ok((handle, ReceiverStream::new(accepted), runtime))
  }
}

impl Drop for RelayRuntime {
  fn drop(&mut self) {
    self.handle.generation.store(0, Ordering::Release);
    self.stop.cancel();
  }
}
impl RelayRuntime {
  pub async fn run(mut self) -> Result<()> {
    let mut generation = 0_u64;
    let mut backoff = Duration::from_millis(100);
    loop {
      let establish = async {
        let control = public_identity(&self.manager.trust(&self.control)?);
        let mut client =
          RelayClient::connect(&self.endpoint, &self.manager.identity, &control).await?;
        Ok::<_, anyhow::Error>((client.open(QUEUE).await?, control.pairing_fingerprint()?))
      };
      let established = tokio::select! {
        _ = self.stop.cancelled() => break,
        result = establish => result,
      };
      if let Ok((connection, fingerprint)) = established {
        generation = generation
          .checked_add(1)
          .context("relay generation exhausted")?;
        self.discard_commands();
        self.handle.generation.store(generation, Ordering::Release);
        let stop = self.stop.child_token();
        let mut session =
          session::Session::new(&self, connection, generation, fingerprint, stop.clone());
        let result = session.run(&mut self.commands).await;
        self.handle.generation.store(0, Ordering::Release);
        stop.cancel();
        session.shutdown().await;
        self.recent = session.recent();
        self.discard_commands();
        if let Err(error) = &result {
          if let Some(status) = error.downcast_ref::<tonic::Status>() {
            tracing::warn!(operation = "relay.transport", outcome = "generation_closed",
              generation, code = ?status.code(), "relay stream failed; unconfirmed operations require reconciliation");
          } else {
            tracing::warn!(operation = "relay.transport", outcome = "generation_closed",
              generation, reason = %error, "relay links closed; unconfirmed operations require reconciliation");
          }
        }
        backoff = Duration::from_millis(100);
      }
      tokio::select! { _ = self.stop.cancelled() => break, _ = tokio::time::sleep(backoff) => {} }
      backoff = (backoff * 2).min(Duration::from_secs(5));
    }
    self.handle.generation.store(0, Ordering::Release);
    self.discard_commands();
    Ok(())
  }
  fn discard_commands(&mut self) {
    while self.commands.try_recv().is_ok() {}
  }
}

fn url_origin(endpoint: &str) -> Result<String> {
  ensure!(
    endpoint.len() <= 2048,
    "relay endpoint exceeds the origin limit"
  );
  let endpoint = Endpoint::from_shared(endpoint.to_owned())
    .map_err(|_| anyhow::anyhow!("invalid relay HTTPS origin"))?;
  let uri = endpoint.uri();
  ensure!(
    uri.scheme_str() == Some("https")
      && uri.authority().is_some()
      && uri.path() == "/"
      && uri.query().is_none()
      && !uri
        .authority()
        .is_some_and(|authority| authority.as_str().contains('@')),
    "relay endpoint must be a credential-free HTTPS origin"
  );
  Ok(uri.to_string())
}
