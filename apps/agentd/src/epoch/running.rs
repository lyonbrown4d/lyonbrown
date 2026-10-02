use std::sync::Arc;
use std::time::Duration;

use agent_app::RuntimeShutdownHandle;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint, SecretToken};
use anyhow::{Context, Result, anyhow};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::model_setup::{ModelSetupCoordinator, PreparedRuntime};
use crate::{AppControllerHost, ManagedRuntimeHost, RuntimeHost, ServeError, serve_authenticated};

type ServerResult = Result<Result<(), ServeError>, tokio::task::JoinError>;
type RuntimeResult = Result<Result<(), agent_app::RuntimeActorError>, tokio::task::JoinError>;

pub(super) struct RunningEpoch {
  pub descriptor: DiscoveryDescriptor,
  server_shutdown: CancellationToken,
  server: Option<JoinHandle<Result<(), ServeError>>>,
  runtime_shutdown: RuntimeShutdownHandle,
  runtime: Option<JoinHandle<Result<(), agent_app::RuntimeActorError>>>,
  stop_timeout: Duration,
}

impl RunningEpoch {
  pub async fn start(
    prepared: PreparedRuntime,
    coordinator: ModelSetupCoordinator,
    scope: ScopeFingerprint,
    stop_timeout: Duration,
  ) -> Result<Self> {
    let setup = async {
      let listener = crate::runtime::bind_loopback()
        .await
        .context("failed to bind replacement epoch")?;
      let address = listener
        .local_addr()
        .context("failed to inspect epoch listener")?;
      let token = SecretToken::generate().context("failed to create epoch credential")?;
      let descriptor = DiscoveryDescriptor::new(
        address,
        std::process::id(),
        Uuid::new_v4(),
        token.clone(),
        scope.clone(),
      )?;
      Ok::<_, anyhow::Error>((listener, token, descriptor))
    }
    .await;
    let (listener, token, descriptor) = match setup {
      Ok(setup) => setup,
      Err(error) => {
        crate::runtime_cleanup::dispose_prepared(prepared, stop_timeout).await;
        return Err(error);
      }
    };
    let runtime_shutdown = prepared.runtime.shutdown_handle();
    let runtime = tokio::spawn(prepared.runtime.join());
    let inner: Arc<dyn RuntimeHost> = Arc::new(AppControllerHost::new(prepared.controller));
    let host: Arc<dyn RuntimeHost> = Arc::new(ManagedRuntimeHost::new(inner, coordinator));
    let server_shutdown = CancellationToken::new();
    let shutdown = server_shutdown.clone();
    let server = tokio::spawn(serve_authenticated(
      listener,
      host,
      token,
      descriptor.server_instance_id(),
      scope,
      shutdown.cancelled_owned(),
    ));
    let mut epoch = Self {
      descriptor,
      server_shutdown,
      server: Some(server),
      runtime_shutdown,
      runtime: Some(runtime),
      stop_timeout,
    };
    tokio::task::yield_now().await;
    if epoch.server.as_ref().is_some_and(JoinHandle::is_finished)
      || epoch.runtime.as_ref().is_some_and(JoinHandle::is_finished)
    {
      let failure = epoch.unexpected_exit().await;
      epoch.stop().await;
      return Err(failure);
    }
    Ok(epoch)
  }

  pub async fn unexpected_exit(&mut self) -> anyhow::Error {
    tokio::select! {
      result = self.server.as_mut().expect("server task present") => {
        self.server = None;
        describe_server_exit(result)
      }
      result = self.runtime.as_mut().expect("runtime task present") => {
        self.runtime = None;
        describe_runtime_exit(result)
      }
    }
  }

  pub fn has_exited(&self) -> bool {
    self.runtime_shutdown.is_shutdown_requested()
      || self.server.as_ref().is_some_and(JoinHandle::is_finished)
      || self.runtime.as_ref().is_some_and(JoinHandle::is_finished)
  }

  pub async fn stop(mut self) {
    self.stop_server().await;
    if let Some(task) = self.runtime.take() {
      crate::runtime_cleanup::stop_joined_runtime(self.runtime_shutdown, task, self.stop_timeout)
        .await;
    }
  }

  async fn stop_server(&mut self) {
    self.server_shutdown.cancel();
    if let Some(mut task) = self.server.take()
      && timeout(self.stop_timeout, &mut task).await.is_err()
    {
      task.abort();
      let _ = timeout(self.stop_timeout, &mut task).await;
    }
  }
}

fn describe_server_exit(result: ServerResult) -> anyhow::Error {
  match result {
    Ok(Ok(())) => anyhow!("agentd epoch server stopped unexpectedly"),
    Ok(Err(error)) => anyhow!(error).context("agentd epoch server failed"),
    Err(error) => anyhow!(error).context("agentd epoch server task failed"),
  }
}

fn describe_runtime_exit(result: RuntimeResult) -> anyhow::Error {
  match result {
    Ok(Ok(())) => anyhow!("agent runtime stopped unexpectedly"),
    Ok(Err(error)) => anyhow!(error).context("agent runtime failed"),
    Err(error) => anyhow!(error).context("agent runtime task failed"),
  }
}
