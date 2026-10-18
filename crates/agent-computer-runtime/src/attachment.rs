//! Authenticated reverse channel embedded in the interactive client process.
#[cfg(feature = "native")]
use std::sync::OnceLock;
use std::{collections::HashSet, sync::Arc, time::Duration};

use agent_client::AgentClient;
#[cfg(feature = "native")]
use agent_computer::{
  DesktopBackend, DesktopSession, DesktopTarget, LeaseRegistry, native::NativeDesktopBackend,
};
use agent_protocol::v1::{ComputerRuntimeRequest, ComputerRuntimeResponse};
use anyhow::{Context, bail, ensure};
use tokio::sync::{mpsc, watch};
use tokio_stream::{Stream, StreamExt, wrappers::ReceiverStream};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
  Command, ComputerRuntime, RUNTIME_PROTOCOL_VERSION, RuntimeHello,
  native_lease::{AttachmentGuard, NativeDesktopLease},
  wire,
};

const CHANNEL_DEADLINE: Duration = Duration::from_secs(5);
const MAX_REQUESTS: usize = 65_536;
type NativeGuard = Option<Arc<NativeDesktopLease>>;

/// Dropping or shutting down closes the channel and cancels future native input.
/// An OS call already running drains on its blocking worker while retaining the
/// runtime and native exclusion; cancellation cannot terminate an OS thread.
pub struct RuntimeAttachment {
  pub host_id: String,
  task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
  status: watch::Receiver<Option<String>>,
  cancellation: CancellationToken,
}
impl RuntimeAttachment {
  pub async fn shutdown(&mut self) -> anyhow::Result<()> {
    self.cancellation.cancel();
    self.wait_closed().await
  }
  /// Cancellation-safe waiting; repeated close/wait does not poll a completed
  /// JoinHandle again or restart the attachment.
  pub async fn wait_closed(&mut self) -> anyhow::Result<()> {
    let result = match self.task.as_mut() {
      Some(task) => task.await.context("computer attachment task unavailable"),
      None => return Ok(()),
    };
    self.task = None;
    result?
  }
  pub fn error(&self) -> Option<String> {
    self.status.borrow().clone()
  }
  pub fn is_finished(&self) -> bool {
    self.task.as_ref().is_none_or(|task| task.is_finished())
  }
}
impl Drop for RuntimeAttachment {
  fn drop(&mut self) {
    self.cancellation.cancel();
    if let Some(task) = &self.task {
      task.abort();
    }
  }
}
/// Read-only native construction. The caller supplies a trusted desktop scope;
/// web content and models cannot create or expand that scope.
#[cfg(feature = "native")]
pub async fn attach_native(
  client: AgentClient,
  daemon_session_id: &str,
  target: DesktopTarget,
) -> anyhow::Result<RuntimeAttachment> {
  let node_id = selected_node(&client, daemon_session_id).await?;
  let construction = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
    let scope = target.scope.clone();
    let mut backend = NativeDesktopBackend::for_target(node_id, target)?;
    let capabilities = backend.capabilities()?;
    ensure!(
      capabilities.can_control(),
      agent_computer::DesktopError::PermissionDenied("desktop control".into())
    );
    let exclusion = NativeDesktopLease::acquire(&capabilities.identity)?;
    let session = DesktopSession {
      id: Uuid::new_v4(),
      identity: capabilities.identity,
      scope,
    };
    static LEASES: OnceLock<Arc<LeaseRegistry>> = OnceLock::new();
    let leases = LEASES
      .get_or_init(|| Arc::new(LeaseRegistry::default()))
      .clone();
    Ok((
      Arc::new(ComputerRuntime::new(session, Box::new(backend), leases)),
      exclusion,
    ))
  });
  let (runtime, exclusion) = tokio::time::timeout(CHANNEL_DEADLINE, construction)
    .await
    .context("native computer construction timed out")?
    .context("native computer construction worker unavailable")??;
  attach_validated(client, daemon_session_id, runtime, Some(exclusion)).await
}

/// Explicit backend injection for embedding and OS-free integration tests.
/// This port does not claim native OS-session exclusion.
pub async fn attach_host(
  client: AgentClient,
  daemon_session_id: &str,
  runtime: Arc<ComputerRuntime>,
) -> anyhow::Result<RuntimeAttachment> {
  let node_id = selected_node(&client, daemon_session_id).await?;
  ensure!(
    runtime.session().identity.node_id == node_id,
    "computer host node identity mismatch"
  );
  attach_validated(client, daemon_session_id, runtime, None).await
}

async fn selected_node(client: &AgentClient, selected: &str) -> anyhow::Result<String> {
  ensure!(
    Uuid::parse_str(selected).is_ok(),
    "invalid daemon session identity"
  );
  let node = tokio::time::timeout(CHANNEL_DEADLINE, client.node())
    .await
    .context("computer daemon handshake timed out")?
    .map_err(|_| anyhow::anyhow!("computer daemon handshake failed"))?;
  let sessions = tokio::time::timeout(CHANNEL_DEADLINE, client.list_sessions())
    .await
    .context("computer session validation timed out")?
    .map_err(|_| anyhow::anyhow!("computer session validation failed"))?;
  ensure!(
    sessions
      .iter()
      .any(|session| session.id == selected && session.owner_node_id == node.id),
    "computer session is not owned by the authenticated daemon"
  );
  Ok(node.id)
}

async fn attach_validated(
  client: AgentClient,
  selected: &str,
  runtime: Arc<ComputerRuntime>,
  exclusion: NativeGuard,
) -> anyhow::Result<RuntimeAttachment> {
  let host_id = Uuid::new_v4().to_string();
  let daemon_session_id = selected.to_owned();
  let session = runtime.session().clone();
  let hello_exclusion = exclusion.clone();
  let payload = tokio::task::spawn_blocking(move || {
    let _exclusion = hello_exclusion;
    wire::encode(&RuntimeHello {
      desktop_session: session,
    })
  })
  .await
  .context("computer hello worker unavailable")??;
  let (sender, receiver) = mpsc::channel(2);
  sender
    .send(response(&host_id, selected, String::new(), payload))
    .await
    .map_err(|_| anyhow::anyhow!("computer hello channel unavailable"))?;
  let requests = tokio::time::timeout(
    CHANNEL_DEADLINE,
    client.attach_computer_runtime(ReceiverStream::new(receiver)),
  )
  .await
  .context("computer attachment handshake timed out")?
  .map_err(|_| anyhow::anyhow!("computer attachment handshake failed"))?;
  let cancellation = CancellationToken::new();
  let root = AttachmentGuard::new(cancellation.clone(), &runtime, exclusion.clone());
  let (status_sender, status) = watch::channel(None);
  let task_host = host_id.clone();
  let task_token = cancellation.clone();
  let task = tokio::spawn(async move {
    let _root = root;
    let result = run_stream(
      requests,
      sender,
      &task_host,
      &daemon_session_id,
      runtime,
      exclusion,
      task_token,
    )
    .await;
    if let Err(error) = &result {
      let _ = status_sender.send(Some(error.to_string()));
    }
    result
  });
  Ok(RuntimeAttachment {
    host_id,
    task: Some(task),
    status,
    cancellation,
  })
}

fn response(
  host: &str,
  session: &str,
  request_id: String,
  payload: Vec<u8>,
) -> ComputerRuntimeResponse {
  ComputerRuntimeResponse {
    request_id,
    host_id: host.into(),
    protocol_version: RUNTIME_PROTOCOL_VERSION,
    payload,
    daemon_session_id: session.into(),
  }
}

async fn run_stream<S, E>(
  mut requests: S,
  sender: mpsc::Sender<ComputerRuntimeResponse>,
  host: &str,
  session: &str,
  runtime: Arc<ComputerRuntime>,
  exclusion: NativeGuard,
  cancellation: CancellationToken,
) -> anyhow::Result<()>
where
  S: Stream<Item = Result<ComputerRuntimeRequest, E>> + Unpin,
{
  let mut seen = HashSet::new();
  loop {
    let request = tokio::select! {
      biased;
      _ = cancellation.cancelled() => return Ok(()),
      request = requests.next() => match request {
        Some(Ok(request)) => request,
        _ => bail!("computer runtime stream disconnected; native actions were not replayed"),
      }
    };
    ensure!(
      request.request_id.len() <= 128,
      "invalid computer request identity"
    );
    let id = Uuid::parse_str(&request.request_id)
      .map_err(|_| anyhow::anyhow!("invalid computer request identity"))?;
    ensure!(!id.is_nil(), "invalid computer request identity");
    ensure!(
      seen.len() < MAX_REQUESTS && seen.insert(id),
      "duplicate computer request or attachment capacity reached"
    );
    ensure!(
      request.payload.len() <= wire::MAX_PACKET_BYTES,
      "computer request packet exceeds limit"
    );
    let worker_runtime = runtime.clone();
    let worker_exclusion = exclusion.clone();
    let worker_token = cancellation.clone();
    let worker_session = session.to_owned();
    let mut worker = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
      let _exclusion = worker_exclusion;
      let command: Command = wire::decode(&request.payload)
        .map_err(|_| anyhow::anyhow!("invalid computer command packet"))?;
      ensure!(
        command.binding.daemon_session_id == worker_session,
        "computer command session mismatch"
      );
      command
        .binding
        .validate()
        .map_err(|_| anyhow::anyhow!("invalid computer command binding"))?;
      let result = worker_runtime.handle(command, worker_token);
      let payload =
        wire::encode(&result).map_err(|_| anyhow::anyhow!("computer response encoding failed"))?;
      Ok((request.request_id, payload))
    });
    let (request_id, payload) = tokio::select! {
      biased;
      _ = cancellation.cancelled() => return Ok(()),
      _ = requests.next() => bail!("computer stream overlapped or disconnected during execution"),
      result = &mut worker => result.map_err(|_| anyhow::anyhow!("computer execution worker unavailable"))??,
    };
    let packet = response(host, session, request_id, payload);
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => return Ok(()),
      _ = requests.next() => bail!("computer stream overlapped or disconnected during response"),
      result = tokio::time::timeout(CHANNEL_DEADLINE, sender.send(packet)) => {
        ensure!(matches!(result, Ok(Ok(()))), "computer response channel stalled or disconnected");
      }
    }
  }
}
