use super::*;
use agent_computer_runtime::wire;
use std::time::Duration;

impl ComputerRouter {
  /// Private transport only. Callers retain run authorization and durable intent.
  pub(in crate::computer) async fn request(
    self: &Arc<Self>,
    pinned: &PinnedHost,
    binding: RunBinding,
    operation: Operation,
    cancellation: CancellationToken,
  ) -> Result<RuntimeResult<Reply>, Status> {
    if cancellation.is_cancelled() {
      return Err(Status::cancelled("native operation cancelled"));
    }
    binding
      .validate()
      .map_err(|_| Status::invalid_argument("invalid native run binding"))?;
    if binding.daemon_session_id != pinned.info.daemon_session_id
      || binding.owner_epoch != pinned.info.owner_epoch
    {
      return Err(Status::failed_precondition("native host binding changed"));
    }
    let expected = Command { binding, operation };
    let payload =
      wire::encode(&expected).map_err(|_| Status::internal("native request encoding failed"))?;
    if payload.len() > 16 * 1024 {
      return Err(Status::resource_exhausted("native request exceeds limit"));
    }
    // Serialize this attachment generation, including background renewal. A
    // queued command has not been dispatched; cancellation here cannot affect
    // the command currently holding the channel or adopt a replacement host.
    let _dispatch = tokio::select! { biased;
      _ = cancellation.cancelled() => return Err(Status::cancelled("native operation cancelled")),
      permit = tokio::time::timeout(Duration::from_secs(35), pinned.dispatch.lock()) => {
        permit.map_err(|_| Status::deadline_exceeded("native dispatch queue deadline"))?
      }
    };
    let id = Uuid::new_v4().to_string();
    let (result, response) = oneshot::channel();
    let outgoing = {
      let mut hosts = self.hosts.lock().await;
      let host = hosts
        .get_mut(&pinned.info.daemon_session_id)
        .filter(|host| matches_host(host, pinned))
        .ok_or_else(disconnected)?;
      if host.pending.is_some() {
        return Err(Status::resource_exhausted("native runtime is busy"));
      }
      host.pending = Some(Pending {
        id: id.clone(),
        expected,
        pinned: pinned.clone(),
        result,
      });
      host.outgoing.clone()
    };
    let mut guard = PendingGuard {
      router: self.clone(),
      pinned: pinned.clone(),
      finished: false,
    };
    let operation = async {
      let permit = outgoing.reserve().await.map_err(|_| disconnected())?;
      {
        let hosts = self.hosts.lock().await;
        let host = hosts
          .get(&pinned.info.daemon_session_id)
          .filter(|host| matches_host(host, pinned))
          .ok_or_else(disconnected)?;
        if !host
          .pending
          .as_ref()
          .is_some_and(|pending| pending.id == id)
        {
          return Err(disconnected());
        }
        permit.send(Ok(v1::ComputerRuntimeRequest {
          request_id: id,
          payload,
        }));
      }
      let result = response.await.map_err(|_| disconnected())?;
      if result.as_ref().is_ok_and(Result::is_ok) {
        let hosts = self.hosts.lock().await;
        if !hosts
          .get(&pinned.info.daemon_session_id)
          .is_some_and(|host| matches_host(host, pinned))
        {
          return Err(disconnected());
        }
      }
      result
    };
    let result = tokio::select! {
      biased;
      _ = cancellation.cancelled() => Err(Status::cancelled("native operation cancelled")),
      result = tokio::time::timeout(Duration::from_secs(35), operation) => {
        result.unwrap_or_else(|_| Err(Status::deadline_exceeded("native operation deadline")))
      }
    };
    if !result.as_ref().is_ok_and(Result::is_ok) {
      self
        .disconnect(&pinned.info.daemon_session_id, pinned.generation)
        .await;
    }
    guard.finished = true;
    result
  }
}

/// Abandoning a dispatched request makes its result unknown; never reuse it.
struct PendingGuard {
  router: Arc<ComputerRouter>,
  pinned: PinnedHost,
  finished: bool,
}

impl Drop for PendingGuard {
  fn drop(&mut self) {
    if !self.finished
      && let Ok(handle) = tokio::runtime::Handle::try_current()
    {
      let router = self.router.clone();
      let pinned = self.pinned.clone();
      handle.spawn(async move {
        router
          .disconnect(&pinned.info.daemon_session_id, pinned.generation)
          .await;
      });
    }
  }
}
