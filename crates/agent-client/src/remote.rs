mod driver;
mod error;
mod receipt;
mod types;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use agent_app::{
  AgentClient as AgentClientPort, AgentClientError, AgentCommand,
  ApprovalDecision as AppApprovalDecision, AvailabilityKind, RuntimeSnapshot,
};
use async_trait::async_trait;
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::epoch::{connected_server, ensure_expected_server};
use crate::{
  AgentClient, ApprovalDecision, AuthenticatedTarget, AuthenticatedTargetResolver, Command,
};
use driver::{DriverContext, establish, run_driver};
use error::status_error;
pub use types::{ConnectionState, RemoteClientOptions, RemoteState, VersionedRemoteSnapshot};

struct RemoteInner {
  commands: Arc<Mutex<AgentClient>>,
  snapshots: watch::Sender<RuntimeSnapshot>,
  remote_snapshots: watch::Sender<VersionedRemoteSnapshot>,
  connection: watch::Sender<ConnectionState>,
  remote_state: watch::Sender<RemoteState>,
  revision: Arc<AtomicU64>,
  cancellation: CancellationToken,
  options: RemoteClientOptions,
}

impl Drop for RemoteInner {
  fn drop(&mut self) {
    self.cancellation.cancel();
  }
}

#[derive(Clone)]
pub struct RemoteAgentClient {
  inner: Arc<RemoteInner>,
}

impl RemoteAgentClient {
  pub async fn connect(
    target: AuthenticatedTarget,
    client_instance_id: impl Into<String>,
    options: RemoteClientOptions,
  ) -> Result<Self, AgentClientError> {
    Self::connect_with_resolver(
      crate::resolver::fixed_target_resolver(target),
      client_instance_id,
      options,
    )
    .await
  }

  pub async fn connect_with_resolver(
    resolver: Arc<dyn AuthenticatedTargetResolver>,
    client_instance_id: impl Into<String>,
    options: RemoteClientOptions,
  ) -> Result<Self, AgentClientError> {
    let client_instance_id = client_instance_id.into();
    let established = establish(&resolver, &client_instance_id, &options).await?;
    let initial_revision = established.snapshot.revision;
    let initial_snapshot = established.snapshot.snapshot.clone();
    let server_instance_id = established.server_instance_id.clone();
    let command_client = established.client.clone();
    let (snapshots, _) = watch::channel(initial_snapshot);
    let (remote_snapshots, _) = watch::channel(VersionedRemoteSnapshot {
      server_instance_id: server_instance_id.clone(),
      revision: initial_revision,
      snapshot: established.snapshot.snapshot.clone(),
    });
    let (connection, _) = watch::channel(ConnectionState::Connected {
      server_instance_id: server_instance_id.clone(),
    });
    let (remote_state, _) = watch::channel(RemoteState {
      connection: connection.borrow().clone(),
      snapshot: Some(remote_snapshots.borrow().clone()),
    });
    let revision = Arc::new(AtomicU64::new(initial_revision));
    let cancellation = CancellationToken::new();
    let commands = Arc::new(Mutex::new(command_client));
    let inner = Arc::new(RemoteInner {
      commands: Arc::clone(&commands),
      snapshots,
      remote_snapshots,
      connection,
      remote_state,
      revision,
      cancellation,
      options: options.clone(),
    });
    tokio::spawn(run_driver(
      established,
      DriverContext {
        resolver,
        commands,
        client_instance_id,
        options,
        snapshots: inner.snapshots.clone(),
        remote_snapshots: inner.remote_snapshots.clone(),
        connection: inner.connection.clone(),
        remote_state: inner.remote_state.clone(),
        revision: Arc::clone(&inner.revision),
        cancellation: inner.cancellation.clone(),
      },
    ));
    Ok(Self { inner })
  }

  pub fn connection_state(&self) -> ConnectionState {
    self.inner.connection.borrow().clone()
  }

  pub fn subscribe_connection(&self) -> watch::Receiver<ConnectionState> {
    self.inner.connection.subscribe()
  }

  pub fn revision(&self) -> u64 {
    self.inner.revision.load(Ordering::Acquire)
  }

  pub fn remote_state(&self) -> RemoteState {
    self.inner.remote_state.borrow().clone()
  }

  /// The authoritative GUI feed atomically couples connection epoch and snapshot.
  pub fn subscribe_remote_state(&self) -> watch::Receiver<RemoteState> {
    self.inner.remote_state.subscribe()
  }

  pub fn remote_snapshot(&self) -> VersionedRemoteSnapshot {
    self.inner.remote_snapshots.borrow().clone()
  }

  /// On reconnect, `ConnectionState::Connected` is published before this atomic resync snapshot.
  pub fn subscribe_remote_snapshots(&self) -> watch::Receiver<VersionedRemoteSnapshot> {
    self.inner.remote_snapshots.subscribe()
  }

  async fn execute_remote(
    &self,
    expected_server_instance_id: &str,
    command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    ensure_expected_server(self.connection_state(), expected_server_instance_id)?;
    let command = command_to_wire(command);
    let command_id = Uuid::new_v4().to_string();
    let server_instance_id = expected_server_instance_id.to_owned();
    let attempts = self.inner.options.command_attempts.max(1);
    for attempt in 0..attempts {
      if self.inner.cancellation.is_cancelled() {
        return Err(AgentClientError::unavailable(
          AvailabilityKind::ShuttingDown,
          "remote client is shutting down",
        ));
      }
      let result = tokio::time::timeout(self.inner.options.rpc_timeout, async {
        let mut client = self.inner.commands.lock().await.clone();
        client
          .execute_with_id_for_server(
            command_id.clone(),
            server_instance_id.clone(),
            command.clone(),
          )
          .await
      })
      .await;
      match result {
        Ok(Ok(response)) => {
          receipt::validate(&response, &command_id)?;
          return Ok(());
        }
        Ok(Err(status)) if is_retryable(&status) && attempt + 1 < attempts => {
          tokio::time::sleep(command_backoff(&self.inner.options, attempt)).await;
        }
        Ok(Err(status)) => return Err(status_error(status)),
        Err(_) if attempt + 1 < attempts => {
          tokio::time::sleep(command_backoff(&self.inner.options, attempt)).await;
        }
        Err(_) => {
          return Err(AgentClientError::unavailable(
            AvailabilityKind::Timeout,
            "agent command deadline exceeded",
          ));
        }
      }
    }
    Err(AgentClientError::unavailable(
      AvailabilityKind::Disconnected,
      "agent command failed after retries",
    ))
  }
}

#[async_trait]
impl AgentClientPort for RemoteAgentClient {
  async fn execute(&self, command: AgentCommand) -> Result<(), AgentClientError> {
    let expected_server_instance_id = connected_server(self.connection_state())?;
    self
      .execute_remote(&expected_server_instance_id, command)
      .await
  }

  async fn execute_for_server(
    &self,
    expected_server_instance_id: &str,
    command: AgentCommand,
  ) -> Result<(), AgentClientError> {
    self
      .execute_remote(expected_server_instance_id, command)
      .await
  }

  fn snapshot(&self) -> RuntimeSnapshot {
    self.inner.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.inner.snapshots.subscribe()
  }
}

fn command_to_wire(command: AgentCommand) -> Command {
  match command {
    AgentCommand::StartTurn { text } => Command::StartTurn { text },
    AgentCommand::SteerCurrentTurn { text } => Command::SteerCurrentTurn { text },
    AgentCommand::QueueFollowUp { text } => Command::QueueFollowUp { text },
    AgentCommand::CancelTurn => Command::CancelTurn,
    AgentCommand::ForceStop => Command::ForceStop,
    AgentCommand::RespondApproval {
      request_id,
      decision,
    } => Command::RespondApproval {
      request_id: request_id.to_string(),
      decision: match decision {
        AppApprovalDecision::AllowOnce => ApprovalDecision::AllowOnce,
        AppApprovalDecision::AllowForSession => ApprovalDecision::AllowForSession,
        AppApprovalDecision::DenyOnce => ApprovalDecision::DenyOnce,
        AppApprovalDecision::DenyRemaining => ApprovalDecision::DenyRemaining,
      },
    },
    AgentCommand::SelectSessionModel { selection } => Command::SelectSessionModel { selection },
    AgentCommand::StartModelDownload { model_id } => Command::StartModelDownload { model_id },
    AgentCommand::CancelModelDownload { operation_id } => {
      Command::CancelModelDownload { operation_id }
    }
  }
}

fn is_retryable(status: &tonic::Status) -> bool {
  matches!(
    status.code(),
    tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
  )
}

fn command_backoff(options: &RemoteClientOptions, attempt: u32) -> Duration {
  driver::bounded_backoff(
    options.reconnect_min_delay,
    options.reconnect_max_delay,
    attempt,
  )
}
