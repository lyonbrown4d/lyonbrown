use super::ComputerRuntimeInfo;
use agent_computer::DesktopCapabilities;
use agent_computer_runtime::{
  Command, Operation, RUNTIME_PROTOCOL_VERSION, Reply, RunBinding, RuntimeResult, wire,
};
use agent_protocol::v1;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tonic::Status;
use uuid::Uuid;

pub(super) struct Pending {
  id: String,
  result: oneshot::Sender<Result<DesktopCapabilities, Status>>,
}
pub(super) struct Host {
  pub info: ComputerRuntimeInfo,
  pub generation: Uuid,
  pub outgoing: mpsc::Sender<Result<v1::ComputerRuntimeRequest, Status>>,
  pub stop: CancellationToken,
  pending: Option<Pending>,
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
    let id = Uuid::new_v4().to_string();
    let payload = wire::encode(&Command {
      binding: binding.clone(),
      operation: Operation::Capabilities,
    })
    .map_err(|_| Status::internal("native request encoding failed"))?;
    let (result, response) = oneshot::channel();
    let (outgoing, generation) = {
      let mut hosts = self.hosts.lock().await;
      let host = hosts
        .get_mut(&binding.daemon_session_id)
        .ok_or_else(disconnected)?;
      if host.info.owner_epoch != binding.owner_epoch {
        return Err(Status::failed_precondition("runtime owner epoch changed"));
      }
      if host.pending.is_some() {
        return Err(Status::resource_exhausted("native runtime is busy"));
      }
      host.pending = Some(Pending {
        id: id.clone(),
        result,
      });
      (host.outgoing.clone(), host.generation)
    };
    let mut guard = PendingGuard {
      router: self.clone(),
      session: binding.daemon_session_id,
      generation,
      finished: false,
    };
    let operation = async {
      outgoing
        .send(Ok(v1::ComputerRuntimeRequest {
          request_id: id,
          payload,
        }))
        .await
        .map_err(|_| disconnected())?;
      response.await.map_err(|_| disconnected())?
    };
    let result = tokio::select! {
      biased;
      _ = cancellation.cancelled() => Err(Status::cancelled("inspection cancelled")),
      result = tokio::time::timeout(Duration::from_secs(35), operation) => result.unwrap_or_else(|_| Err(Status::deadline_exceeded("native inspection deadline"))),
    };
    if result.is_ok() {
      guard.finished = true;
    }
    // On failure Drop disconnects only this host generation; callers never replay.
    result
  }

  pub(super) async fn receive(
    &self,
    session: &str,
    generation: Uuid,
    response: v1::ComputerRuntimeResponse,
  ) -> Result<(), Status> {
    {
      let hosts = self.hosts.lock().await;
      let host = hosts
        .get(session)
        .filter(|host| host.generation == generation)
        .ok_or_else(disconnected)?;
      if response.payload.len() > 4096
        || response.host_id != host.info.host_id
        || response.daemon_session_id != session
        || response.protocol_version != RUNTIME_PROTOCOL_VERSION
        || !host
          .pending
          .as_ref()
          .is_some_and(|pending| pending.id == response.request_id)
      {
        return Err(Status::invalid_argument("native response binding mismatch"));
      }
    }
    let decoded =
      tokio::task::spawn_blocking(move || wire::decode::<RuntimeResult<Reply>>(&response.payload))
        .await
        .map_err(|_| Status::internal("native response decoding failed"))?
        .map_err(|_| Status::invalid_argument("invalid native response"))?;
    let mut hosts = self.hosts.lock().await;
    let host = hosts
      .get_mut(session)
      .filter(|host| host.generation == generation)
      .ok_or_else(disconnected)?;
    let capabilities = match decoded {
      Ok(Reply::Capabilities(capabilities))
        if capabilities.identity == host.info.desktop_session.identity =>
      {
        Ok(capabilities)
      }
      Err(_) => Err(Status::failed_precondition("native inspection rejected")),
      _ => return Err(Status::invalid_argument("unexpected native reply")),
    };
    let pending = host.pending.take().ok_or_else(disconnected)?;
    let _ = pending.result.send(capabilities);
    Ok(())
  }
}

struct PendingGuard {
  router: Arc<ComputerRouter>,
  session: String,
  generation: Uuid,
  finished: bool,
}
impl Drop for PendingGuard {
  fn drop(&mut self) {
    if !self.finished
      && let Ok(handle) = tokio::runtime::Handle::try_current()
    {
      let router = self.router.clone();
      let session = self.session.clone();
      let generation = self.generation;
      handle.spawn(async move {
        router.disconnect(&session, generation).await;
      });
    }
  }
}
fn disconnected() -> Status {
  Status::unavailable("native runtime disconnected; reconnect explicitly")
}
