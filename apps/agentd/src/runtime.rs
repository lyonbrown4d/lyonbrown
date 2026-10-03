use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_app::{AppController, RuntimeTask, approval_channel, spawn_runtime_waiting_for_model};
use agent_kernel::Kernel;
use agent_launch::{LaunchRequest, scoped_run_directory};
use agent_local::{
  DiscoveryStore, LifecycleLock, ScopeFingerprint, SecretToken, SecureRunDirectory,
};
use agent_runtime::config::{CliOverrides, LoadOptions, RuntimeConfig};
use agent_tool::SessionApprovalPolicy;
use anyhow::{Context, Result, anyhow, bail};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::args::Args;
use crate::epoch::{EpochSupervisor, StoreDescriptorPublisher};
use crate::model_setup::{
  ModelSetupCoordinator, PreparedRuntime, RuntimeModelInstaller, SetupPlan, inspect_setup_plan,
};
use crate::{AppControllerHost, serve_authenticated};

pub(crate) const COMMAND_CAPACITY: usize = 32;
const APPROVAL_CAPACITY: usize = 4;
const EPOCH_STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub use crate::epoch::{
  MemoryProbe, ProductionRuntimeActivator, build_active_runtime, catalog_failure_is_fatal,
};
pub use crate::model_setup::managed_runtime_config;

#[derive(Debug, Clone)]
pub struct DaemonPaths {
  workspace: PathBuf,
  home: PathBuf,
  run_directory: PathBuf,
  scope_fingerprint: ScopeFingerprint,
}

impl DaemonPaths {
  pub fn resolve(workspace: &Path, home: &Path, runtime_fingerprint: &str) -> Result<Self> {
    let workspace = workspace
      .canonicalize()
      .with_context(|| format!("workspace is unavailable: {}", workspace.display()))?;
    let home = std::path::absolute(home).context("failed to resolve home directory")?;
    let request = LaunchRequest::new(
      workspace.clone(),
      runtime_fingerprint,
      false,
      Duration::from_secs(1),
    )?;
    let run_directory = scoped_run_directory(&home, &workspace, request.scope_fingerprint())?;
    Ok(Self {
      workspace,
      home,
      run_directory,
      scope_fingerprint: request.scope_fingerprint().clone(),
    })
  }

  pub fn workspace(&self) -> &Path {
    &self.workspace
  }

  pub fn home(&self) -> &Path {
    &self.home
  }

  pub fn run_directory(&self) -> &Path {
    &self.run_directory
  }

  pub fn scope_fingerprint(&self) -> &ScopeFingerprint {
    &self.scope_fingerprint
  }
}

pub fn open_secure_run_directory(path: &Path) -> Result<SecureRunDirectory> {
  SecureRunDirectory::open(path).map_err(|error| anyhow!(error))
}

pub async fn bind_loopback() -> std::io::Result<TcpListener> {
  TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await
}

pub async fn serve_runtime_until<F>(
  listener: TcpListener,
  controller: AppController,
  runtime: RuntimeTask,
  token: SecretToken,
  server_instance_id: Uuid,
  scope_fingerprint: ScopeFingerprint,
  shutdown: F,
) -> Result<()>
where
  F: Future<Output = ()> + Send + 'static,
{
  let host = Arc::new(AppControllerHost::new(controller));
  let server_shutdown = CancellationToken::new();
  let external_shutdown = server_shutdown.clone();
  let shutdown_task = tokio::spawn(async move {
    shutdown.await;
    external_shutdown.cancel();
  });
  let server = serve_authenticated(
    listener,
    host,
    token,
    server_instance_id,
    scope_fingerprint,
    server_shutdown.clone().cancelled_owned(),
  );
  let runtime = runtime.join();
  tokio::pin!(server);
  tokio::pin!(runtime);

  let result = tokio::select! {
    server_result = &mut server => {
      let runtime_result = runtime.await;
      match server_result.context("agentd gRPC server stopped unexpectedly") {
        Ok(()) => runtime_result.context("agent runtime stopped unexpectedly"),
        Err(error) => Err(error),
      }
    }
    runtime_result = &mut runtime => {
      server_shutdown.cancel();
      let server_result = server.await;
      match runtime_result.context("agent runtime stopped unexpectedly") {
        Ok(()) => server_result.context("agentd gRPC server stopped unexpectedly"),
        Err(error) => Err(error),
      }
    }
  };
  shutdown_task.abort();
  result?;
  Ok(())
}

pub async fn run(args: Args) -> Result<()> {
  let home = dirs::home_dir().context("home directory is unavailable")?;
  let paths = DaemonPaths::resolve(&args.workspace, &home, &args.runtime_config_fingerprint)?;
  let run_directory = open_secure_run_directory(paths.run_directory())?;
  let lifecycle = LifecycleLock::try_acquire(&run_directory)?
    .ok_or_else(|| anyhow!("another agentd already owns this workspace runtime"))?;
  let config = load_runtime_config(&args, &paths)?;
  validate_runtime_fingerprint(&config, &args.runtime_config_fingerprint)?;
  let setup_plan = inspect_setup_plan(&config, &paths).await?;
  let activator = Arc::new(ProductionRuntimeActivator::new(
    config.clone(),
    paths.clone(),
    &setup_plan,
  ));
  let (coordinator, mut replacements) = ModelSetupCoordinator::from_plan(
    setup_plan.clone(),
    Arc::new(RuntimeModelInstaller),
    activator,
  );
  let (controller, runtime) = build_runtime_from_setup_plan(config, &paths, &setup_plan).await?;
  let publisher = StoreDescriptorPublisher::new(DiscoveryStore::new(run_directory));
  let mut supervisor = EpochSupervisor::new(
    publisher,
    coordinator,
    paths.scope_fingerprint.clone(),
    EPOCH_STOP_TIMEOUT,
  );
  supervisor
    .start_initial(PreparedRuntime::new(controller, runtime))
    .await?;
  let result = supervisor
    .supervise(&mut replacements, shutdown_signal())
    .await;
  drop(lifecycle);
  result
}

#[doc(hidden)]
pub async fn build_runtime(
  config: RuntimeConfig,
  paths: &DaemonPaths,
) -> Result<(AppController, RuntimeTask)> {
  let setup_plan = inspect_setup_plan(&config, paths).await?;
  build_runtime_from_setup_plan(config, paths, &setup_plan).await
}

#[doc(hidden)]
pub async fn build_runtime_from_setup_plan(
  config: RuntimeConfig,
  paths: &DaemonPaths,
  setup_plan: &SetupPlan,
) -> Result<(AppController, RuntimeTask)> {
  let Some(config) = setup_plan.runtime_config(config)? else {
    let kernel = Kernel::builder().workspace_root(paths.workspace()).build();
    return Ok(spawn_runtime_waiting_for_model(kernel, COMMAND_CAPACITY));
  };

  let prepared = build_active_runtime(config, paths).await?;
  Ok((prepared.controller, prepared.runtime))
}

pub fn load_runtime_config(args: &Args, paths: &DaemonPaths) -> Result<RuntimeConfig> {
  let mut options = LoadOptions::for_workspace(paths.workspace());
  options.global_config = paths.home().join(".agent").join("config.toml");
  options.explicit_config = args.config.clone();
  options.cli = CliOverrides {
    profile: args.profile.clone(),
    provider: args.provider.clone(),
    model: args.model.clone(),
    endpoint: args.endpoint.clone(),
    approve_all: args.approve_all.then_some(true),
    deny_all: args.deny_all.then_some(true),
    trust_workspace_mcp: args.trust_workspace_mcp,
    trust_workspace_config: args.trust_workspace_config,
    ..CliOverrides::default()
  };
  RuntimeConfig::load(options).context("failed to load daemon configuration")
}

pub fn validate_runtime_fingerprint(config: &RuntimeConfig, expected: &str) -> Result<()> {
  let actual = config
    .fingerprint()
    .context("failed to fingerprint daemon configuration")?;
  if actual == expected {
    Ok(())
  } else {
    bail!("daemon runtime configuration does not match the client fingerprint")
  }
}

pub(crate) fn approval_runtime(
  config: &RuntimeConfig,
) -> (SessionApprovalPolicy, Option<agent_app::ApprovalInbox>) {
  if config.approval.approve_all {
    (SessionApprovalPolicy::approve_all(), None)
  } else if config.approval.deny_all {
    (SessionApprovalPolicy::deny_all(), None)
  } else {
    let (prompt, inbox) = approval_channel(APPROVAL_CAPACITY);
    (
      SessionApprovalPolicy::interactive_with_timeout(
        Arc::new(prompt),
        Duration::from_secs(config.approval.timeout_seconds),
      ),
      Some(inbox),
    )
  }
}

async fn shutdown_signal() {
  if tokio::signal::ctrl_c().await.is_err() {
    tracing::error!(
      error_code = "shutdown_signal_failed",
      "failed to install shutdown signal handler"
    );
  }
}
