use super::ComputerRuntimeInfo;
use agent_computer::DesktopCapabilities;
use agent_computer_runtime::{Command, Operation, Reply, RunBinding, RuntimeResult};
use agent_protocol::v1;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tonic::Status;
use uuid::Uuid;

mod receive;
mod request;
#[cfg(test)]
mod tests;
mod validation;

#[derive(Clone)]
pub(super) struct PinnedHost {
  pub info: ComputerRuntimeInfo,
  pub generation: Uuid,
  dispatch: Arc<Mutex<()>>,
}

pub(super) struct Pending {
  id: String,
  expected: Command,
  pinned: PinnedHost,
  result: oneshot::Sender<Result<RuntimeResult<Reply>, Status>>,
}
pub(super) struct Host {
  pub info: ComputerRuntimeInfo,
  pub generation: Uuid,
  pub outgoing: mpsc::Sender<Result<v1::ComputerRuntimeRequest, Status>>,
  pub stop: CancellationToken,
  pending: Option<Pending>,
  dispatch: Arc<Mutex<()>>,
}
#[derive(Default)]
pub(crate) struct ComputerRouter {
  hosts: Mutex<HashMap<String, Host>>,
}
impl ComputerRouter {
  pub(super) async fn register(
    &self,
    info: ComputerRuntimeInfo,
    outgoing: mpsc::Sender<Result<v1::ComputerRuntimeRequest, Status>>,
    stop: CancellationToken,
  ) -> Result<Uuid, Status> {
    let mut hosts = self.hosts.lock().await;
    // Response-stream drop cancels before its reader finishes cleanup. Release
    // those disconnected generations before checking replacement or capacity.
    hosts.retain(|_, host| !host.stop.is_cancelled());
    if hosts.len() >= 8
      || hosts.contains_key(&info.daemon_session_id)
      || hosts.values().any(|host| host.info.host_id == info.host_id)
    {
      return Err(Status::resource_exhausted(
        "native runtime already attached or capacity reached",
      ));
    }
    let generation = Uuid::new_v4();
    hosts.insert(
      info.daemon_session_id.clone(),
      Host {
        info,
        generation,
        outgoing,
        stop,
        pending: None,
        dispatch: Arc::new(Mutex::new(())),
      },
    );
    Ok(generation)
  }

  pub(super) async fn info(&self, session: &str, epoch: u64) -> Option<ComputerRuntimeInfo> {
    self
      .hosts
      .lock()
      .await
      .get(session)
      .filter(|host| host.info.owner_epoch == epoch && !host.stop.is_cancelled())
      .map(|host| host.info.clone())
  }

  pub(super) async fn disconnect(&self, session: &str, generation: Uuid) {
    let mut hosts = self.hosts.lock().await;
    if hosts
      .get(session)
      .is_some_and(|host| host.generation == generation)
      && let Some(host) = hosts.remove(session)
    {
      host.stop.cancel();
      if let Some(pending) = host.pending {
        let _ = pending.result.send(Err(disconnected()));
      }
    }
  }

  pub(super) async fn capabilities(
    self: &Arc<Self>,
    binding: RunBinding,
    cancellation: CancellationToken,
  ) -> Result<DesktopCapabilities, Status> {
    if cancellation.is_cancelled() {
      return Err(Status::cancelled("inspection cancelled"));
    }
    let host = self
      .pin(&binding.daemon_session_id, binding.owner_epoch)
      .await?;
    match self
      .request(&host, binding, Operation::Capabilities, cancellation)
      .await?
    {
      Ok(Reply::Capabilities(capabilities)) => Ok(capabilities),
      Err(_) => Err(Status::failed_precondition("native inspection rejected")),
      _ => Err(Status::invalid_argument("unexpected native reply")),
    }
  }

  pub(super) async fn pin(&self, session: &str, epoch: u64) -> Result<PinnedHost, Status> {
    let hosts = self.hosts.lock().await;
    let host = hosts.get(session).ok_or_else(disconnected)?;
    if host.info.owner_epoch != epoch || host.stop.is_cancelled() {
      return Err(Status::failed_precondition("runtime owner epoch changed"));
    }
    Ok(PinnedHost {
      info: host.info.clone(),
      generation: host.generation,
      dispatch: host.dispatch.clone(),
    })
  }
}

fn matches_host(host: &Host, pinned: &PinnedHost) -> bool {
  host.generation == pinned.generation
    && host.info.host_id == pinned.info.host_id
    && host.info.daemon_session_id == pinned.info.daemon_session_id
    && host.info.owner_epoch == pinned.info.owner_epoch
    && host.info.desktop_session == pinned.info.desktop_session
    && !host.stop.is_cancelled()
}

fn disconnected() -> Status {
  Status::unavailable("native runtime disconnected; reconnect explicitly")
}
