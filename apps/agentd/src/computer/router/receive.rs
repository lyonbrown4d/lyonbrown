use super::*;
use agent_computer_runtime::{RUNTIME_PROTOCOL_VERSION, wire};

impl ComputerRouter {
  pub(in crate::computer) async fn receive(
    &self,
    session: &str,
    generation: Uuid,
    response: v1::ComputerRuntimeResponse,
  ) -> Result<(), Status> {
    let result = self.receive_inner(session, generation, response).await;
    if result.is_err() {
      self.disconnect(session, generation).await;
    }
    result
  }

  async fn receive_inner(
    &self,
    session: &str,
    generation: Uuid,
    response: v1::ComputerRuntimeResponse,
  ) -> Result<(), Status> {
    let (expected, pinned) = {
      let hosts = self.hosts.lock().await;
      let host = hosts
        .get(session)
        .filter(|host| host.generation == generation)
        .ok_or_else(disconnected)?;
      let pending = host.pending.as_ref().ok_or_else(disconnected)?;
      if !matches_host(host, &pending.pinned)
        || response.payload.len() > response_limit(&pending.expected.operation)
        || response.host_id != pending.pinned.info.host_id
        || response.daemon_session_id != session
        || response.protocol_version != RUNTIME_PROTOCOL_VERSION
        || pending.id != response.request_id
      {
        return Err(Status::invalid_argument("native response binding mismatch"));
      }
      (pending.expected.clone(), pending.pinned.clone())
    };
    let request_id = response.request_id;
    let validation_host = pinned.clone();
    let decoded = tokio::task::spawn_blocking(move || {
      let decoded = wire::decode::<RuntimeResult<Reply>>(&response.payload)
        .map_err(|_| Status::invalid_argument("invalid native response"))?;
      if let Ok(reply) = &decoded {
        validation::validate(&validation_host, &expected, reply)?;
      }
      Ok::<_, Status>(decoded)
    })
    .await
    .map_err(|_| Status::internal("native response decoding failed"))??;
    let failed = decoded.is_err();
    {
      let mut hosts = self.hosts.lock().await;
      let host = hosts
        .get_mut(session)
        .filter(|host| matches_host(host, &pinned))
        .ok_or_else(disconnected)?;
      if !host
        .pending
        .as_ref()
        .is_some_and(|pending| pending.id == request_id)
      {
        return Err(Status::invalid_argument("native response request changed"));
      }
      let pending = host.pending.take().ok_or_else(disconnected)?;
      let _ = pending.result.send(Ok(decoded));
    }
    if failed {
      self.disconnect(session, generation).await;
    }
    Ok(())
  }
}

fn response_limit(operation: &Operation) -> usize {
  match operation {
    Operation::Capabilities => 4096,
    Operation::Observe { .. } => wire::MAX_PACKET_BYTES,
    _ => 16 * 1024,
  }
}
