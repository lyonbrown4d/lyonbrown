use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_app::{
  AgentClient as AgentClientPort, AgentCommand, RuntimeCommandError, RuntimePhase, spawn_runtime,
};
use agent_client::{
  AgentClient as GrpcAgentClient, AuthenticatedTarget, AuthenticatedTargetResolver,
  ConnectionState, RemoteAgentClient, RemoteClientOptions,
};
use agent_kernel::Kernel;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint};
use agent_runtime::config::{LoadOptions, RuntimeConfig};
use agentd::epoch::{CutoverOutcome, DescriptorPublisher, EpochSupervisor};
use agentd::model_setup::{
  ModelInstaller, ModelSetupCoordinator, PreparedRuntime, RuntimeActivator,
};
use agentd::runtime::DaemonPaths;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct RecordingPublisher {
  current: Mutex<Option<DiscoveryDescriptor>>,
  published: Mutex<Vec<DiscoveryDescriptor>>,
  fail_next: Mutex<bool>,
  fail_after_write: Mutex<bool>,
}

impl RecordingPublisher {
  fn fail_next(&self) {
    *self.fail_next.lock().unwrap() = true;
  }

  fn fail_after_write(&self) {
    *self.fail_after_write.lock().unwrap() = true;
  }

  fn history(&self) -> Vec<DiscoveryDescriptor> {
    self.published.lock().unwrap().clone()
  }
}

impl DescriptorPublisher for RecordingPublisher {
  fn publish(&self, descriptor: &DiscoveryDescriptor) -> Result<()> {
    if std::mem::take(&mut *self.fail_next.lock().unwrap()) {
      return Err(anyhow!("injected publication failure"));
    }
    *self.current.lock().unwrap() = Some(descriptor.clone());
    self.published.lock().unwrap().push(descriptor.clone());
    if std::mem::take(&mut *self.fail_after_write.lock().unwrap()) {
      return Err(anyhow!("injected failure after publication"));
    }
    Ok(())
  }

  fn read(&self) -> Result<DiscoveryDescriptor> {
    self
      .current
      .lock()
      .unwrap()
      .clone()
      .ok_or_else(|| anyhow!("descriptor not published"))
  }
}

#[async_trait]
impl AuthenticatedTargetResolver for RecordingPublisher {
  async fn resolve(&self) -> Result<AuthenticatedTarget, agent_client::AgentClientError> {
    let descriptor = self.read().map_err(|error| {
      agent_client::AgentClientError::unavailable(
        agent_client::AvailabilityKind::Disconnected,
        error.to_string(),
      )
    })?;
    Ok(AuthenticatedTarget::from_descriptor(&descriptor))
  }
}

struct UnusedInstaller;

#[async_trait]
impl ModelInstaller for UnusedInstaller {
  async fn install(
    &self,
    _cache_dir: &std::path::Path,
    _preset: &'static agent_runtime::model_manager::ModelPreset,
    _progress: watch::Sender<agent_runtime::model_manager::ModelDownloadProgress>,
    _install_gate: agent_runtime::model_manager::InstallPhaseGate,
    _cancellation: CancellationToken,
  ) -> Result<std::path::PathBuf> {
    unreachable!("disabled model setup cannot install")
  }
}

struct UnusedActivator;

#[async_trait]
impl RuntimeActivator for UnusedActivator {
  async fn activate(&self, _installed_model: std::path::PathBuf) -> Result<PreparedRuntime> {
    unreachable!("disabled model setup cannot activate")
  }
}

async fn coordinator() -> ModelSetupCoordinator {
  let workspace = tempfile::tempdir().unwrap().keep();
  let home = tempfile::tempdir().unwrap().keep();
  let mut options = LoadOptions::for_workspace(&workspace);
  options.global_config = home.join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.model.provider = "ollama".into();
  let fingerprint = config.fingerprint().unwrap();
  let paths = DaemonPaths::resolve(&workspace, &home, &fingerprint).unwrap();
  ModelSetupCoordinator::from_runtime(
    &config,
    &paths,
    Arc::new(UnusedInstaller),
    Arc::new(UnusedActivator),
  )
  .await
  .unwrap()
  .0
}

fn prepared() -> (PreparedRuntime, agent_app::AppController) {
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  (
    PreparedRuntime::new(controller.clone(), runtime),
    controller,
  )
}

async fn handshake(descriptor: &DiscoveryDescriptor) -> agent_protocol::v1::HandshakeResponse {
  let target = AuthenticatedTarget::from_descriptor(descriptor);
  let mut client = GrpcAgentClient::connect(target, "epoch-cutover")
    .await
    .unwrap();
  client.handshake("epoch-cutover", "test").await.unwrap()
}

#[tokio::test]
async fn old_rpc_remains_available_while_replacement_is_being_prepared() {
  let publisher = Arc::new(RecordingPublisher::default());
  let scope = ScopeFingerprint::parse(&"ef".repeat(32)).unwrap();
  let mut supervisor = EpochSupervisor::new(
    publisher.clone(),
    coordinator().await,
    scope,
    Duration::from_millis(250),
  );
  supervisor.start_initial(prepared().0).await.unwrap();
  let old = publisher.read().unwrap();
  let (release, building) = tokio::sync::oneshot::channel::<()>();
  let build = tokio::spawn(async move {
    building.await.unwrap();
    prepared().0
  });

  handshake(&old).await;
  release.send(()).unwrap();
  supervisor.cutover(build.await.unwrap()).await.unwrap();
  supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn cutover_rotates_transport_identity_but_preserves_process_scope() {
  let publisher = Arc::new(RecordingPublisher::default());
  let scope = ScopeFingerprint::parse(&"ab".repeat(32)).unwrap();
  let mut supervisor = EpochSupervisor::new(
    publisher.clone(),
    coordinator().await,
    scope.clone(),
    Duration::from_millis(250),
  );
  supervisor.start_initial(prepared().0).await.unwrap();
  let first = publisher.read().unwrap();
  handshake(&first).await;

  assert_eq!(
    supervisor.cutover(prepared().0).await.unwrap(),
    CutoverOutcome::Switched
  );
  let second = publisher.read().unwrap();
  let response = handshake(&second).await;

  assert_ne!(first.address(), second.address());
  assert_ne!(first.token(), second.token());
  assert_ne!(first.server_instance_id(), second.server_instance_id());
  assert_eq!(first.pid(), second.pid());
  assert_eq!(first.scope_fingerprint(), second.scope_fingerprint());
  assert_eq!(
    response.server_instance_id,
    second.server_instance_id().to_string()
  );
  assert_eq!(publisher.history().len(), 2);
  supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn publication_failure_keeps_old_epoch_and_disposes_replacement() {
  let publisher = Arc::new(RecordingPublisher::default());
  let scope = ScopeFingerprint::parse(&"cd".repeat(32)).unwrap();
  let mut supervisor = EpochSupervisor::new(
    publisher.clone(),
    coordinator().await,
    scope,
    Duration::from_millis(250),
  );
  supervisor.start_initial(prepared().0).await.unwrap();
  let old = publisher.read().unwrap();
  let (replacement, replacement_controller) = prepared();
  publisher.fail_next();

  assert_eq!(
    supervisor.cutover(replacement).await.unwrap(),
    CutoverOutcome::KeptCurrent
  );
  handshake(&old).await;
  tokio::time::timeout(Duration::from_secs(1), async {
    loop {
      if replacement_controller
        .execute(AgentCommand::StartTurn { text: "x".into() })
        .await
        == Err(RuntimeCommandError::RuntimeStopped)
      {
        break;
      }
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("rejected replacement must be stopped");
  assert_eq!(publisher.read().unwrap(), old);

  publisher.fail_after_write();
  assert_eq!(
    supervisor.cutover(prepared().0).await.unwrap(),
    CutoverOutcome::KeptCurrent
  );
  assert_eq!(publisher.read().unwrap(), old);
  handshake(&old).await;
  supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn resolver_client_reconnects_to_ready_replacement_epoch() {
  let publisher = Arc::new(RecordingPublisher::default());
  let scope = ScopeFingerprint::parse(&"12".repeat(32)).unwrap();
  let mut supervisor = EpochSupervisor::new(
    publisher.clone(),
    coordinator().await,
    scope,
    Duration::from_millis(250),
  );
  supervisor.start_initial(prepared().0).await.unwrap();
  let resolver: Arc<dyn AuthenticatedTargetResolver> = publisher.clone();
  let client = RemoteAgentClient::connect_with_resolver(
    resolver,
    "epoch-reconnect",
    RemoteClientOptions::for_tests(),
  )
  .await
  .unwrap();
  let mut state = client.subscribe_remote_state();

  supervisor.cutover(prepared().0).await.unwrap();
  let replacement_id = publisher.read().unwrap().server_instance_id().to_string();
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let ready = matches!(
        &state.borrow().connection,
        ConnectionState::Connected { server_instance_id } if server_instance_id == &replacement_id
      ) && state
        .borrow()
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.snapshot.phase == RuntimePhase::Ready);
      if ready {
        break;
      }
      state.changed().await.unwrap();
    }
  })
  .await
  .expect("client should reconnect to replacement epoch");
  assert_eq!(client.snapshot().phase, RuntimePhase::Ready);
  supervisor.shutdown().await.unwrap();
}
