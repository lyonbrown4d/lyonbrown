mod model_setup_support;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_app::{AgentCommand, AppController, RuntimeCommandError, spawn_runtime};
use agent_kernel::Kernel;
use agentd::model_setup::{ModelSetupCoordinator, PreparedRuntime, RuntimeActivator};
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Semaphore;

use model_setup_support::{FakeInstaller, Harness, InstallMode, preset_id};

struct BlockingActivator {
  started: Semaphore,
  release: Semaphore,
  created: Semaphore,
  controller: Mutex<Option<AppController>>,
}

impl BlockingActivator {
  fn new() -> Arc<Self> {
    Arc::new(Self {
      started: Semaphore::new(0),
      release: Semaphore::new(0),
      created: Semaphore::new(0),
      controller: Mutex::new(None),
    })
  }
}

#[async_trait]
impl RuntimeActivator for BlockingActivator {
  async fn activate(&self, _installed_model: PathBuf) -> Result<PreparedRuntime> {
    self.started.add_permits(1);
    self.release.acquire().await.unwrap().forget();
    let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
    *self.controller.lock().unwrap() = Some(controller.clone());
    self.created.add_permits(1);
    Ok(PreparedRuntime::new(controller, runtime))
  }
}

#[tokio::test]
async fn shutdown_closes_late_replacement_race_and_joins_the_producer() {
  let base = Harness::managed(InstallMode::WaitForCancel, false).await;
  let installer = FakeInstaller::new(InstallMode::Success);
  let activator = BlockingActivator::new();
  let (coordinator, mut replacements) =
    ModelSetupCoordinator::from_runtime(&base.config, &base.paths, installer, activator.clone())
      .await
      .unwrap();
  coordinator.start(preset_id().into()).await.unwrap();
  tokio::time::timeout(Duration::from_secs(2), activator.started.acquire())
    .await
    .unwrap()
    .unwrap()
    .forget();
  replacements.close();
  let shutdown = tokio::spawn({
    let coordinator = coordinator.clone();
    async move { coordinator.shutdown().await }
  });
  tokio::task::yield_now().await;
  activator.release.add_permits(1);
  tokio::time::timeout(Duration::from_secs(2), activator.created.acquire())
    .await
    .expect("late runtime must be constructed")
    .unwrap()
    .forget();

  tokio::time::timeout(Duration::from_secs(2), shutdown)
    .await
    .expect("coordinator shutdown must be bounded")
    .unwrap();
  assert!(replacements.recv().await.is_none());
  let controller = activator
    .controller
    .lock()
    .unwrap()
    .clone()
    .expect("barrier guarantees a late runtime");
  assert_eq!(
    controller
      .execute(AgentCommand::StartTurn { text: "x".into() })
      .await,
    Err(RuntimeCommandError::RuntimeStopped)
  );
}
