#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use agent_app::{AppController, RuntimePhase, RuntimeSnapshot, spawn_runtime_waiting_for_model};
use agent_kernel::Kernel;
use agent_runtime::config::{LoadOptions, RuntimeConfig};
use agent_runtime::model_manager::{
  DownloadPhase, InstallPhaseGate, ModelDownloadProgress, ModelPreset, default_preset,
};
use agentd::model_setup::{
  ModelInstaller, ModelSetupCoordinator, PreparedRuntime, RuntimeActivator,
};
use agentd::runtime::DaemonPaths;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tempfile::TempDir;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
pub enum InstallMode {
  WaitForCancel,
  Success,
  SuccessAfterCancel,
  Installing,
  InstallingAfterRelease,
  Failure,
  FailureAfterReleaseThenWait,
  CloseProgressWaitForCancel,
}

pub struct FakeInstaller {
  mode: InstallMode,
  pub calls: AtomicUsize,
  pub cancellations: AtomicUsize,
  pub started: Semaphore,
  pub release: Semaphore,
}

impl FakeInstaller {
  pub fn new(mode: InstallMode) -> Arc<Self> {
    Arc::new(Self {
      mode,
      calls: AtomicUsize::new(0),
      cancellations: AtomicUsize::new(0),
      started: Semaphore::new(0),
      release: Semaphore::new(0),
    })
  }

  pub async fn wait_started(&self) {
    self.started.acquire().await.unwrap().forget();
  }

  pub fn release_one(&self) {
    self.release.add_permits(1);
  }
}

#[async_trait]
impl ModelInstaller for FakeInstaller {
  async fn install(
    &self,
    cache_dir: &Path,
    preset: &'static ModelPreset,
    progress: watch::Sender<ModelDownloadProgress>,
    install_gate: InstallPhaseGate,
    cancellation: CancellationToken,
  ) -> Result<PathBuf> {
    let call = self.calls.fetch_add(1, Ordering::SeqCst);
    self.started.add_permits(1);
    match self.mode {
      InstallMode::WaitForCancel => {
        cancellation.cancelled().await;
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        Err(anyhow!("cancelled internal secret path C:/private/token"))
      }
      InstallMode::Success => {
        progress.send_replace(ModelDownloadProgress::new(
          DownloadPhase::Downloading,
          50,
          Some(100),
        ));
        install_gate.wait(&cancellation).await?;
        tokio::task::yield_now().await;
        Ok(cache_dir.join(preset.filename))
      }
      InstallMode::SuccessAfterCancel => {
        cancellation.cancelled().await;
        Ok(cache_dir.join(preset.filename))
      }
      InstallMode::Installing => {
        progress.send_replace(ModelDownloadProgress::new(
          DownloadPhase::Installing,
          100,
          Some(100),
        ));
        install_gate.wait(&cancellation).await?;
        std::future::pending().await
      }
      InstallMode::InstallingAfterRelease => {
        self.release.acquire().await.unwrap().forget();
        progress.send_replace(ModelDownloadProgress::new(
          DownloadPhase::Installing,
          100,
          Some(100),
        ));
        install_gate.wait(&cancellation).await?;
        std::future::pending().await
      }
      InstallMode::Failure => Err(anyhow!(
        "download failed at C:/private/model with Bearer secret-token"
      )),
      InstallMode::FailureAfterReleaseThenWait if call == 0 => {
        self.release.acquire().await.unwrap().forget();
        Err(anyhow!("first attempt failed"))
      }
      InstallMode::FailureAfterReleaseThenWait => {
        cancellation.cancelled().await;
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        Err(anyhow!("second attempt cancelled"))
      }
      InstallMode::CloseProgressWaitForCancel => {
        drop(progress);
        cancellation.cancelled().await;
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        Err(anyhow!("cancelled after progress closed"))
      }
    }
  }
}

pub struct FakeActivator {
  fail: bool,
  pub calls: AtomicUsize,
  last_controller: Mutex<Option<AppController>>,
}

impl FakeActivator {
  pub fn success() -> Arc<Self> {
    Arc::new(Self {
      fail: false,
      calls: AtomicUsize::new(0),
      last_controller: Mutex::new(None),
    })
  }

  pub fn failure() -> Arc<Self> {
    Arc::new(Self {
      fail: true,
      calls: AtomicUsize::new(0),
      last_controller: Mutex::new(None),
    })
  }

  pub fn last_controller(&self) -> Option<AppController> {
    self.last_controller.lock().unwrap().clone()
  }
}

#[async_trait]
impl RuntimeActivator for FakeActivator {
  async fn activate(&self, _installed_model: PathBuf) -> Result<PreparedRuntime> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    if self.fail {
      return Err(anyhow!(
        "activation leaked token secret-token at C:/private/model"
      ));
    }
    let kernel = Kernel::builder().build();
    let (controller, runtime) = spawn_runtime_waiting_for_model(kernel, 2);
    *self.last_controller.lock().unwrap() = Some(controller.clone());
    Ok(PreparedRuntime::new(controller, runtime))
  }
}

pub struct Harness {
  pub coordinator: ModelSetupCoordinator,
  pub replacements: mpsc::UnboundedReceiver<PreparedRuntime>,
  pub installer: Arc<FakeInstaller>,
  pub activator: Arc<FakeActivator>,
  pub config: RuntimeConfig,
  pub paths: DaemonPaths,
  _workspace: TempDir,
  _home: TempDir,
}

impl Harness {
  pub async fn managed(mode: InstallMode, activation_fails: bool) -> Self {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let config = default_config(workspace.path(), home.path());
    let paths = daemon_paths(&config, workspace.path(), home.path());
    let installer = FakeInstaller::new(mode);
    let activator = if activation_fails {
      FakeActivator::failure()
    } else {
      FakeActivator::success()
    };
    let (coordinator, replacements) =
      ModelSetupCoordinator::from_runtime(&config, &paths, installer.clone(), activator.clone())
        .await
        .unwrap();
    Self {
      coordinator,
      replacements,
      installer,
      activator,
      config,
      paths,
      _workspace: workspace,
      _home: home,
    }
  }
}

pub fn default_config(workspace: &Path, home: &Path) -> RuntimeConfig {
  let mut options = LoadOptions::for_workspace(workspace);
  options.global_config = home.join("missing.toml");
  options.environment = Some(HashMap::new());
  RuntimeConfig::load(options).unwrap()
}

pub fn daemon_paths(config: &RuntimeConfig, workspace: &Path, home: &Path) -> DaemonPaths {
  DaemonPaths::resolve(workspace, home, &config.fingerprint().unwrap()).unwrap()
}

pub fn preset_id() -> &'static str {
  default_preset().id
}

pub fn waiting_snapshot() -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    ..RuntimeSnapshot::default()
  }
}
