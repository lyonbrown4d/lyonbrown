mod driver;

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use agent_app::{
  AgentClient as AgentClientPort, AgentClientError, AgentCommand, AvailabilityKind, RuntimeSnapshot,
};
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::{
  AuthenticatedTarget, AuthenticatedTargetResolver, ConnectionState, RemoteClientOptions,
  RemoteState,
};
use driver::{DriverContext, run};

const COMMAND_CAPACITY: usize = 32;

pub type NativeCommandFuture =
  Pin<Box<dyn Future<Output = Result<(), AgentClientError>> + Send + 'static>>;

struct CommandRequest {
  command: AgentCommand,
  response: oneshot::Sender<Result<(), AgentClientError>>,
}

struct NativeInner {
  commands: mpsc::Sender<CommandRequest>,
  snapshots: watch::Sender<RuntimeSnapshot>,
  remote_state: watch::Sender<RemoteState>,
  cancellation: CancellationToken,
  running: Arc<AtomicBool>,
}

impl Drop for NativeInner {
  fn drop(&mut self) {
    self.cancellation.cancel();
  }
}

#[derive(Clone)]
pub struct NativeAgentClient {
  inner: Arc<NativeInner>,
}

#[derive(Clone)]
pub struct NativeDriverMonitor {
  running: Arc<AtomicBool>,
}

impl NativeDriverMonitor {
  pub fn is_running(&self) -> bool {
    self.running.load(Ordering::Acquire)
  }
}

impl NativeAgentClient {
  pub fn start(
    target: AuthenticatedTarget,
    client_instance_id: impl Into<String>,
    options: RemoteClientOptions,
  ) -> io::Result<Self> {
    Self::start_with_resolver(
      crate::resolver::fixed_target_resolver(target),
      client_instance_id,
      options,
    )
  }

  pub fn start_with_resolver(
    resolver: Arc<dyn AuthenticatedTargetResolver>,
    client_instance_id: impl Into<String>,
    options: RemoteClientOptions,
  ) -> io::Result<Self> {
    let client_instance_id = client_instance_id.into();
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (snapshots, _) = watch::channel(RuntimeSnapshot::default());
    let (remote_state, _) = watch::channel(RemoteState {
      connection: ConnectionState::Connecting,
      snapshot: None,
    });
    let cancellation = CancellationToken::new();
    let running = Arc::new(AtomicBool::new(true));
    let context = DriverContext {
      resolver,
      client_instance_id,
      options,
      commands: command_rx,
      snapshots: snapshots.clone(),
      remote_state: remote_state.clone(),
      cancellation: cancellation.clone(),
      running: Arc::clone(&running),
    };
    if let Err(error) = std::thread::Builder::new()
      .name("agent-native-client".into())
      .spawn(move || run(context))
    {
      running.store(false, Ordering::Release);
      return Err(error);
    }
    Ok(Self {
      inner: Arc::new(NativeInner {
        commands,
        snapshots,
        remote_state,
        cancellation,
        running,
      }),
    })
  }

  pub fn remote_state(&self) -> RemoteState {
    self.inner.remote_state.borrow().clone()
  }

  pub fn subscribe_remote_state(&self) -> watch::Receiver<RemoteState> {
    self.inner.remote_state.subscribe()
  }

  pub fn driver_monitor(&self) -> NativeDriverMonitor {
    NativeDriverMonitor {
      running: Arc::clone(&self.inner.running),
    }
  }

  pub fn dispatch(&self, command: AgentCommand) -> NativeCommandFuture {
    if self.inner.cancellation.is_cancelled() {
      return ready_error(
        AvailabilityKind::ShuttingDown,
        "native client is shutting down",
      );
    }
    match self.inner.remote_state.borrow().connection.clone() {
      ConnectionState::Connected { .. } => {}
      ConnectionState::Failed { error, .. } => return ready_result(Err(error)),
      _ => {
        return ready_error(
          AvailabilityKind::Disconnected,
          "agent daemon is not connected",
        );
      }
    }
    let (response, receiver) = oneshot::channel();
    let request = CommandRequest { command, response };
    match self.inner.commands.try_send(request) {
      Ok(()) => Box::pin(async move {
        receiver.await.unwrap_or_else(|_| {
          Err(AgentClientError::unavailable(
            AvailabilityKind::ShuttingDown,
            "native client driver stopped",
          ))
        })
      }),
      Err(mpsc::error::TrySendError::Full(_)) => ready_error(
        AvailabilityKind::Disconnected,
        "native command queue is full",
      ),
      Err(mpsc::error::TrySendError::Closed(_)) => ready_error(
        AvailabilityKind::ShuttingDown,
        "native client driver stopped",
      ),
    }
  }
}

#[async_trait]
impl AgentClientPort for NativeAgentClient {
  async fn execute(&self, command: AgentCommand) -> Result<(), AgentClientError> {
    self.dispatch(command).await
  }

  fn snapshot(&self) -> RuntimeSnapshot {
    self.inner.snapshots.borrow().clone()
  }

  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot> {
    self.inner.snapshots.subscribe()
  }
}

fn ready_error(kind: AvailabilityKind, message: &'static str) -> NativeCommandFuture {
  ready_result(Err(AgentClientError::unavailable(kind, message)))
}

fn ready_result(result: Result<(), AgentClientError>) -> NativeCommandFuture {
  Box::pin(async move { result })
}
