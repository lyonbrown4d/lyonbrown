mod rpc;
mod validation;

use std::sync::Arc;

use agent_app::AgentCommand;
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::{CommandResponse, RequestMetadata};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tonic::{Response, Status};
use uuid::Uuid;

use crate::RuntimeHost;
use crate::replay::{Fingerprint, ReplayCache, ReplayDecision};
use validation::{map_command_error, validate_metadata};

pub struct AgentGrpcService {
  pub(super) host: Arc<dyn RuntimeHost>,
  replay_cache: Arc<Mutex<ReplayCache>>,
  pub(super) server_instance_id: String,
  pub(super) stream_shutdown: CancellationToken,
}

impl AgentGrpcService {
  pub fn new(host: Arc<dyn RuntimeHost>) -> Self {
    Self::with_replay_capacity(host, 1024)
  }

  pub fn with_replay_capacity(host: Arc<dyn RuntimeHost>, capacity: usize) -> Self {
    Self {
      host,
      replay_cache: Arc::new(Mutex::new(ReplayCache::new(capacity))),
      server_instance_id: Uuid::new_v4().to_string(),
      stream_shutdown: CancellationToken::new(),
    }
  }

  pub fn server_instance_id(&self) -> &str {
    &self.server_instance_id
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
      return Err(Status::failed_precondition(format!(
        "server instance changed: command expected {}, active instance is {}",
        metadata.expected_server_instance_id, self.server_instance_id
      )));
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
      stream_shutdown: self.stream_shutdown.clone(),
    }
  }
}
