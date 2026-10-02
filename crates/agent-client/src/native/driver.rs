use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agent_app::{AgentClient as _, AgentClientError, AvailabilityKind, ProtocolErrorKind};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use super::CommandRequest;
use crate::{
  AuthenticatedTargetResolver, ConnectionState, RemoteAgentClient, RemoteClientOptions, RemoteState,
};

pub(super) struct DriverContext {
  pub resolver: Arc<dyn AuthenticatedTargetResolver>,
  pub client_instance_id: String,
  pub options: RemoteClientOptions,
  pub commands: mpsc::Receiver<CommandRequest>,
  pub snapshots: watch::Sender<agent_app::RuntimeSnapshot>,
  pub remote_state: watch::Sender<RemoteState>,
  pub cancellation: CancellationToken,
  pub running: Arc<AtomicBool>,
}

struct RunningGuard(Arc<AtomicBool>);

impl Drop for RunningGuard {
  fn drop(&mut self) {
    self.0.store(false, Ordering::Release);
  }
}

pub(super) fn run(context: DriverContext) {
  let _running = RunningGuard(Arc::clone(&context.running));
  let runtime = match tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
  {
    Ok(runtime) => runtime,
    Err(error) => {
      publish_failed(
        &context,
        AgentClientError::protocol(ProtocolErrorKind::Unsupported, error.to_string()),
      );
      drain_commands(context.commands, shutdown_error());
      return;
    }
  };
  runtime.block_on(run_async(context));
}

async fn run_async(mut context: DriverContext) {
  let Some(remote) = connect(&context).await else {
    drain_commands(context.commands, shutdown_error());
    return;
  };
  context.remote_state.send_replace(remote.remote_state());
  context.snapshots.send_replace(remote.snapshot());
  let mut remote_states = remote.subscribe_remote_state();
  let mut snapshots = remote.subscribe();
  let mut terminal_error = shutdown_error();
  loop {
    tokio::select! {
      _ = context.cancellation.cancelled() => break,
      changed = remote_states.changed() => {
        if changed.is_err() {
          terminal_error = disconnected_error();
          break;
        }
        let state = remote_states.borrow().clone();
        let failed = matches!(state.connection, ConnectionState::Failed { .. });
        context.remote_state.send_replace(state);
        if failed {
          terminal_error = disconnected_error();
          break;
        }
      }
      changed = snapshots.changed() => {
        if changed.is_ok() {
          context.snapshots.send_replace(snapshots.borrow().clone());
        }
      }
      request = context.commands.recv() => match request {
        Some(request) => {
          let response = tokio::select! {
            _ = context.cancellation.cancelled() => Err(shutdown_error()),
            result = remote.execute(request.command) => result,
          };
          let cancelled = context.cancellation.is_cancelled();
          let _ = request.response.send(response);
          if cancelled {
            break;
          }
        }
        None => break,
      }
    }
  }
  drop(remote);
  drain_commands(context.commands, terminal_error);
}

async fn connect(context: &DriverContext) -> Option<RemoteAgentClient> {
  let mut attempt = 0u32;
  loop {
    let result = tokio::select! {
      _ = context.cancellation.cancelled() => return None,
      result = connect_remote(context) => result,
    };
    match result {
      Ok(remote) => return Some(remote),
      Err(error @ AgentClientError::Protocol { .. }) => {
        publish_failed(context, error);
        return None;
      }
      Err(error) => {
        attempt = attempt.saturating_add(1);
        context.remote_state.send_replace(RemoteState {
          connection: ConnectionState::Reconnecting {
            server_instance_id: None,
            attempt,
            error,
          },
          snapshot: None,
        });
        let delay = bounded_backoff(&context.options, attempt - 1);
        tokio::select! {
          _ = context.cancellation.cancelled() => return None,
          _ = tokio::time::sleep(delay) => {}
        }
      }
    }
  }
}

async fn connect_remote(context: &DriverContext) -> Result<RemoteAgentClient, AgentClientError> {
  RemoteAgentClient::connect_with_resolver(
    Arc::clone(&context.resolver),
    &context.client_instance_id,
    context.options.clone(),
  )
  .await
}

fn publish_failed(context: &DriverContext, error: AgentClientError) {
  context.remote_state.send_replace(RemoteState {
    connection: ConnectionState::Failed {
      server_instance_id: None,
      error,
    },
    snapshot: None,
  });
}

fn bounded_backoff(options: &RemoteClientOptions, attempt: u32) -> Duration {
  let factor = 1u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
  options
    .reconnect_min_delay
    .saturating_mul(factor)
    .min(options.reconnect_max_delay)
}

fn drain_commands(mut commands: mpsc::Receiver<CommandRequest>, error: AgentClientError) {
  while let Ok(request) = commands.try_recv() {
    let _ = request.response.send(Err(error.clone()));
  }
}

fn shutdown_error() -> AgentClientError {
  AgentClientError::unavailable(
    AvailabilityKind::ShuttingDown,
    "native client is shutting down",
  )
}

fn disconnected_error() -> AgentClientError {
  AgentClientError::unavailable(AvailabilityKind::Disconnected, "agent daemon disconnected")
}
