use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use agent_app::spawn_runtime;
use agent_kernel::Kernel;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint};
use agent_runtime::config::{LoadOptions, RuntimeConfig};
use agentd::epoch::{DescriptorPublisher, EpochSupervisor};
use agentd::model_setup::{
  ModelInstaller, ModelSetupCoordinator, PreparedRuntime, RuntimeActivator,
};
use agentd::runtime::DaemonPaths;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

struct PublishGate {
  current: Mutex<Option<DiscoveryDescriptor>>,
  count: Mutex<usize>,
  entered: std::sync::mpsc::Sender<()>,
  release: Arc<(Mutex<bool>, Condvar)>,
}

impl PublishGate {
  fn new(entered: std::sync::mpsc::Sender<()>, release: Arc<(Mutex<bool>, Condvar)>) -> Self {
    Self {
      current: Mutex::new(None),
      count: Mutex::new(0),
      entered,
      release,
    }
  }
}

impl DescriptorPublisher for PublishGate {
  fn publish(&self, descriptor: &DiscoveryDescriptor) -> Result<()> {
    let mut count = self.count.lock().unwrap();
    *count += 1;
    let should_wait = *count == 2;
    drop(count);
    if should_wait {
      self.entered.send(()).unwrap();
      let (lock, wake) = &*self.release;
      let mut released = lock.lock().unwrap();
      while !*released {
        released = wake.wait(released).unwrap();
      }
    }
    *self.current.lock().unwrap() = Some(descriptor.clone());
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
    unreachable!()
  }
}

struct UnusedActivator;

#[async_trait]
impl RuntimeActivator for UnusedActivator {
  async fn activate(&self, _installed_model: std::path::PathBuf) -> Result<PreparedRuntime> {
    unreachable!()
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
  let paths = DaemonPaths::resolve(&workspace, &home, &config.fingerprint().unwrap()).unwrap();
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

fn prepared() -> PreparedRuntime {
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  PreparedRuntime::new(controller, runtime)
}

async fn setup() -> (
  EpochSupervisor,
  Arc<PublishGate>,
  DiscoveryDescriptor,
  std::sync::mpsc::Receiver<()>,
  Arc<(Mutex<bool>, Condvar)>,
  agent_app::RuntimeShutdownHandle,
) {
  let (entered_tx, entered_rx) = std::sync::mpsc::channel();
  let release = Arc::new((Mutex::new(false), Condvar::new()));
  let publisher = Arc::new(PublishGate::new(entered_tx, release.clone()));
  let mut supervisor = EpochSupervisor::new(
    publisher.clone(),
    coordinator().await,
    ScopeFingerprint::parse(&"56".repeat(32)).unwrap(),
    Duration::from_millis(250),
  );
  let initial = prepared();
  let shutdown = initial.runtime.shutdown_handle();
  supervisor.start_initial(initial).await.unwrap();
  let descriptor = publisher.read().unwrap();
  (
    supervisor, publisher, descriptor, entered_rx, release, shutdown,
  )
}

fn release_publish(release: &Arc<(Mutex<bool>, Condvar)>) {
  let (lock, wake) = &**release;
  *lock.lock().unwrap() = true;
  wake.notify_all();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_during_publication_rolls_back_the_replacement_descriptor() {
  let (supervisor, publisher, initial, entered, release, _) = setup().await;
  let (replacement_tx, mut replacement_rx) = mpsc::unbounded_channel();
  replacement_tx.send(prepared()).unwrap();
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let task = tokio::spawn(async move {
    let mut supervisor = supervisor;
    supervisor
      .supervise(&mut replacement_rx, async move {
        let _ = shutdown_rx.await;
      })
      .await
  });
  tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(2)).unwrap())
    .await
    .unwrap();

  shutdown_tx.send(()).unwrap();
  release_publish(&release);

  task.await.unwrap().unwrap();
  assert_eq!(publisher.read().unwrap(), initial);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_runtime_exit_during_publication_rolls_back_and_fails_closed() {
  let (supervisor, publisher, initial, entered, release, old_runtime) = setup().await;
  let (replacement_tx, mut replacement_rx) = mpsc::unbounded_channel();
  replacement_tx.send(prepared()).unwrap();
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let mut task = tokio::spawn(async move {
    let mut supervisor = supervisor;
    supervisor
      .supervise(&mut replacement_rx, async move {
        let _ = shutdown_rx.await;
      })
      .await
  });
  tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(2)).unwrap())
    .await
    .unwrap();

  old_runtime.abort();
  release_publish(&release);
  tokio::time::sleep(Duration::from_millis(100)).await;
  let exited = task.is_finished();
  if !exited {
    shutdown_tx.send(()).unwrap();
  }
  let result = (&mut task).await.unwrap();

  assert!(exited, "old runtime exit must interrupt cutover");
  assert!(result.is_err());
  assert_eq!(publisher.read().unwrap(), initial);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacement_exit_during_publication_rolls_back_and_keeps_old_epoch() {
  let (supervisor, publisher, initial, entered, release, _) = setup().await;
  let (replacement_tx, mut replacement_rx) = mpsc::unbounded_channel();
  let replacement = prepared();
  let replacement_runtime = replacement.runtime.shutdown_handle();
  replacement_tx.send(replacement).unwrap();
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let task = tokio::spawn(async move {
    let mut supervisor = supervisor;
    supervisor
      .supervise(&mut replacement_rx, async move {
        let _ = shutdown_rx.await;
      })
      .await
  });
  tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(2)).unwrap())
    .await
    .unwrap();

  replacement_runtime.abort();
  release_publish(&release);
  tokio::time::sleep(Duration::from_millis(100)).await;
  shutdown_tx.send(()).unwrap();

  task.await.unwrap().unwrap();
  assert_eq!(publisher.read().unwrap(), initial);
}
