mod rpc;
mod validation;

use std::sync::Arc;

use agent_app::AgentCommand;
use agent_local::ScopeFingerprint;
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::{CommandResponse, RequestMetadata};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tonic::{Response, Status};
use uuid::Uuid;

use crate::RuntimeHost;
use crate::replay::{Fingerprint, ReplayCache, ReplayDecision};
use validation::{epoch_mismatch_status, map_command_error, validate_metadata};

pub(crate) struct AgentGrpcService {
  pub(super) host: Arc<dyn RuntimeHost>,
  replay_cache: Arc<Mutex<ReplayCache>>,
  pub(super) server_instance_id: String,
  pub(super) scope_fingerprint: ScopeFingerprint,
  pub(super) stream_shutdown: CancellationToken,
}

impl AgentGrpcService {
  pub(crate) fn new_with_identity(
    host: Arc<dyn RuntimeHost>,
    server_instance_id: Uuid,
    scope_fingerprint: ScopeFingerprint,
  ) -> Self {
    Self::with_replay_capacity_and_identity(host, 1024, server_instance_id, scope_fingerprint)
  }

  fn with_replay_capacity_and_identity(
    host: Arc<dyn RuntimeHost>,
    capacity: usize,
    server_instance_id: Uuid,
    scope_fingerprint: ScopeFingerprint,
  ) -> Self {
    assert!(
      !server_instance_id.is_nil(),
      "server instance identity must not be nil"
    );
    Self {
      host,
      replay_cache: Arc::new(Mutex::new(ReplayCache::new(capacity))),
      server_instance_id: server_instance_id.to_string(),
      scope_fingerprint,
      stream_shutdown: CancellationToken::new(),
    }
  }

  pub(crate) fn with_stream_shutdown(mut self, shutdown: CancellationToken) -> Self {
    self.stream_shutdown = shutdown;
    self
  }

  async fn execute(
    &self,
    metadata: Option<RequestMetadata>,
    command: AgentCommand,
    fingerprint: Fingerprint,
  ) -> Result<Response<CommandResponse>, Status> {
    let metadata = validate_metadata(metadata)?;
    if metadata.expected_server_instance_id != self.server_instance_id {
      return Err(epoch_mismatch_status(
        &metadata.expected_server_instance_id,
        &self.server_instance_id,
      ));
    }
    let key = (
      metadata.client_instance_id.clone(),
      metadata.command_id.clone(),
    );
    let decision = self
      .replay_cache
      .lock()
      .await
      .reserve(key.clone(), fingerprint);
    let result = match decision {
      ReplayDecision::Replay(result) => result,
      ReplayDecision::Conflict => {
        return Err(Status::already_exists(
          "command_id was already used for a different command payload",
        ));
      }
      ReplayDecision::Wait(waiter) => waiter.wait().await,
      ReplayDecision::Execute(waiter) => {
        let host = Arc::clone(&self.host);
        let replay_cache = Arc::clone(&self.replay_cache);
        tokio::spawn(async move {
          let result = host.execute(command).await;
          replay_cache.lock().await.complete(key, result);
        });
        waiter.wait().await
      }
    };
    result.map_err(map_command_error)?;
    Ok(Response::new(CommandResponse {
      protocol_version: PROTOCOL_VERSION,
      command_id: metadata.command_id,
      accepted: true,
    }))
  }
}

impl Clone for AgentGrpcService {
  fn clone(&self) -> Self {
    Self {
      host: Arc::clone(&self.host),
      replay_cache: Arc::clone(&self.replay_cache),
      server_instance_id: self.server_instance_id.clone(),
      scope_fingerprint: self.scope_fingerprint.clone(),
      stream_shutdown: self.stream_shutdown.clone(),
    }
  }
}
