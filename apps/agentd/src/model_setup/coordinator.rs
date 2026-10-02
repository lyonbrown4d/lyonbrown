use std::path::PathBuf;
use std::sync::Arc;

use agent_app::{ModelSetupPhase, ModelSetupView, RuntimeCommandError};
use agent_runtime::config::RuntimeConfig;
use anyhow::Result;
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::model_setup::plan::{SetupPlan, SetupTarget, inspect_setup_plan};
use crate::model_setup::{ModelInstaller, PreparedRuntime, RuntimeActivator};
use crate::runtime::DaemonPaths;

#[derive(Clone)]
pub struct ModelSetupCoordinator {
  pub(super) inner: Arc<Inner>,
}

pub(super) struct Inner {
  pub(super) target: SetupTarget,
  pub(super) cache_dir: PathBuf,
  pub(super) installer: Arc<dyn ModelInstaller>,
  pub(super) activator: Arc<dyn RuntimeActivator>,
  pub(super) state: Mutex<State>,
  pub(super) snapshots: watch::Sender<Option<ModelSetupView>>,
  pub(super) replacements: mpsc::UnboundedSender<PreparedRuntime>,
  shutdown: CancellationToken,
  operation_task: Mutex<Option<JoinHandle<()>>>,
}

pub(super) struct State {
  pub(super) active: Option<ActiveOperation>,
  pub(super) last_successful_activation: Option<String>,
}

pub(super) struct ActiveOperation {
  pub(super) id: String,
  pub(super) phase: ModelSetupPhase,
  pub(super) cancellation: CancellationToken,
}

impl ModelSetupCoordinator {
  pub async fn from_runtime(
    config: &RuntimeConfig,
    paths: &DaemonPaths,
    installer: Arc<dyn ModelInstaller>,
    activator: Arc<dyn RuntimeActivator>,
  ) -> Result<(Self, mpsc::UnboundedReceiver<PreparedRuntime>)> {
    let plan = inspect_setup_plan(config, paths).await?;
    Ok(Self::from_plan(plan, installer, activator))
  }

  #[doc(hidden)]
  pub fn from_plan(
    plan: SetupPlan,
    installer: Arc<dyn ModelInstaller>,
    activator: Arc<dyn RuntimeActivator>,
  ) -> (Self, mpsc::UnboundedReceiver<PreparedRuntime>) {
    let (snapshots, _) = watch::channel(plan.initial);
    let (replacements, receiver) = mpsc::unbounded_channel();
    (
      Self {
        inner: Arc::new(Inner {
          target: plan.target,
          cache_dir: plan.cache_dir,
          installer,
          activator,
          state: Mutex::new(State {
            active: None,
            last_successful_activation: None,
          }),
          snapshots,
          replacements,
          shutdown: CancellationToken::new(),
          operation_task: Mutex::new(None),
        }),
      },
      receiver,
    )
  }

  pub fn subscribe(&self) -> watch::Receiver<Option<ModelSetupView>> {
    self.inner.snapshots.subscribe()
  }

  pub fn snapshot(&self) -> Option<ModelSetupView> {
    self.inner.snapshots.borrow().clone()
  }

  pub fn advertises_download(&self) -> bool {
    matches!(self.inner.target, SetupTarget::Managed(_))
  }

  pub async fn start(&self, model_id: String) -> Result<(), RuntimeCommandError> {
    if self.inner.shutdown.is_cancelled() {
      return Err(RuntimeCommandError::ModelDownloadUnavailable);
    }
    let preset = match self.inner.target {
      SetupTarget::Managed(preset) => preset,
      SetupTarget::Disabled | SetupTarget::Unavailable => {
        return Err(RuntimeCommandError::ModelDownloadUnavailable);
      }
    };
    if model_id != preset.id {
      return Err(RuntimeCommandError::InvalidModelId {
        requested: model_id,
        configured: preset.id.into(),
      });
    }
    let mut state = self.inner.state.lock().await;
    if self.inner.shutdown.is_cancelled() {
      return Err(RuntimeCommandError::ModelDownloadUnavailable);
    }
    if state.active.is_some() {
      return Ok(());
    }
    if self
      .snapshot()
      .is_some_and(|view| view.phase == ModelSetupPhase::Ready)
    {
      return Err(RuntimeCommandError::ModelDownloadUnavailable);
    }
    let operation_id = Uuid::new_v4().to_string();
    let cancellation = CancellationToken::new();
    state.active = Some(ActiveOperation {
      id: operation_id.clone(),
      phase: ModelSetupPhase::WaitingForLock,
      cancellation: cancellation.clone(),
    });
    self.publish_progress(
      preset,
      &operation_id,
      ModelSetupPhase::WaitingForLock,
      0,
      Some(preset.download_bytes),
    );
    let coordinator = self.clone();
    let task = tokio::spawn(async move {
      coordinator
        .run_install(preset, operation_id, cancellation)
        .await;
    });
    let previous = self.inner.operation_task.lock().await.replace(task);
    drop(state);
    if let Some(previous) = previous {
      let _ = previous.await;
    }
    Ok(())
  }

  pub async fn shutdown(&self) {
    self.inner.shutdown.cancel();
    let mut state = self.inner.state.lock().await;
    if let Some(active) = state.active.take() {
      active.cancellation.cancel();
    }
    let task = self.inner.operation_task.lock().await.take();
    drop(state);
    if let Some(task) = task {
      crate::runtime_cleanup::stop_background_task(
        task,
        crate::runtime_cleanup::DEFAULT_RUNTIME_STOP_TIMEOUT,
      )
      .await;
    }
  }

  pub async fn cancel(&self, operation_id: String) -> Result<(), RuntimeCommandError> {
    let state = self.inner.state.lock().await;
    let active = state
      .active
      .as_ref()
      .ok_or(RuntimeCommandError::NoActiveModelDownload)?;
    if active.id != operation_id {
      return Err(RuntimeCommandError::ModelDownloadOperationMismatch {
        expected: active.id.clone(),
        received: operation_id,
      });
    }
    if matches!(
      active.phase,
      ModelSetupPhase::Installing | ModelSetupPhase::Activating
    ) {
      return Err(RuntimeCommandError::ModelDownloadCancellationTooLate);
    }
    active.cancellation.cancel();
    Ok(())
  }
}
