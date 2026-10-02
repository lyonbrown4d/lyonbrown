use agent_app::{ModelSetupAction, ModelSetupFailureView, ModelSetupPhase};
use agent_runtime::model_manager::{
  DownloadPhase, ModelDownloadProgress, ModelPreset, install_phase_gate,
};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use super::ModelSetupCoordinator;
use super::plan::managed_view;

impl ModelSetupCoordinator {
  pub(super) async fn run_install(
    &self,
    preset: &'static ModelPreset,
    operation_id: String,
    cancellation: CancellationToken,
  ) {
    let (progress, mut observed) = watch::channel(ModelDownloadProgress::new(
      DownloadPhase::WaitingForLock,
      0,
      Some(preset.download_bytes),
    ));
    let (install_gate, mut install_requests) = install_phase_gate();
    let install = self.inner.installer.install(
      &self.inner.cache_dir,
      preset,
      progress,
      install_gate,
      cancellation.clone(),
    );
    tokio::pin!(install);
    let mut progress_open = true;
    let mut install_gate_open = true;
    let result = loop {
      tokio::select! {
        result = &mut install => break result,
        changed = observed.changed(), if progress_open => {
          if changed.is_err() {
            progress_open = false;
          } else {
            let progress = observed.borrow_and_update().clone();
            self.update_progress(preset, &operation_id, progress).await;
          }
        }
        request = install_requests.recv(), if install_gate_open => {
          match request {
            Some(permit) => {
              let progress = observed.borrow().clone();
              self.enter_installing(
                preset,
                &operation_id,
                &cancellation,
                progress,
                permit,
              ).await;
            }
            None => install_gate_open = false,
          }
        }
      }
    };
    match result {
      Ok(path) => {
        self
          .activate(preset, &operation_id, &cancellation, path)
          .await
      }
      Err(_error) if cancellation.is_cancelled() => {
        self.finish_cancelled(preset, &operation_id).await;
        tracing::debug!(
          operation_id,
          model_id = preset.id,
          error_code = "cancelled",
          "managed model download cancelled"
        );
      }
      Err(_error) => {
        tracing::error!(
          operation_id,
          model_id = preset.id,
          error_code = "download_failed",
          "managed model download failed"
        );
        self
          .finish_failed(preset, &operation_id, "download_failed")
          .await;
      }
    }
  }

  async fn update_progress(
    &self,
    preset: &'static ModelPreset,
    operation_id: &str,
    progress: ModelDownloadProgress,
  ) {
    let phase = match progress.phase {
      DownloadPhase::WaitingForLock => ModelSetupPhase::WaitingForLock,
      DownloadPhase::Checking => ModelSetupPhase::Checking,
      DownloadPhase::Downloading => ModelSetupPhase::Downloading,
      DownloadPhase::Verifying => ModelSetupPhase::Verifying,
      DownloadPhase::Installing => return,
      DownloadPhase::Ready => return,
    };
    let mut state = self.inner.state.lock().await;
    let Some(active) = state
      .active
      .as_mut()
      .filter(|active| active.id == operation_id)
    else {
      return;
    };
    active.phase = phase;
    self.publish_progress(
      preset,
      operation_id,
      phase,
      progress.downloaded_bytes,
      progress.total_bytes,
    );
  }

  async fn enter_installing(
    &self,
    preset: &'static ModelPreset,
    operation_id: &str,
    cancellation: &CancellationToken,
    progress: ModelDownloadProgress,
    permit: oneshot::Sender<bool>,
  ) {
    let mut state = self.inner.state.lock().await;
    let active = state
      .active
      .as_mut()
      .filter(|active| active.id == operation_id);
    let allowed = if let Some(active) = active {
      if cancellation.is_cancelled() {
        false
      } else {
        active.phase = ModelSetupPhase::Installing;
        self.publish_progress(
          preset,
          operation_id,
          ModelSetupPhase::Installing,
          progress.downloaded_bytes,
          progress.total_bytes,
        );
        true
      }
    } else {
      false
    };
    let _ = permit.send(allowed);
  }

  async fn finish_cancelled(&self, preset: &'static ModelPreset, operation_id: &str) {
    let mut state = self.inner.state.lock().await;
    if !state
      .active
      .as_ref()
      .is_some_and(|active| active.id == operation_id)
    {
      return;
    }
    state.active = None;
    self.publish_cancelled(preset, operation_id);
  }

  pub(super) fn publish_cancelled(&self, preset: &'static ModelPreset, operation_id: &str) {
    let view = managed_view(
      preset,
      ModelSetupPhase::Cancelled,
      Some(operation_id.into()),
      0,
      None,
      vec![ModelSetupAction::Retry],
    );
    self.inner.snapshots.send_replace(Some(view));
  }

  pub(super) async fn finish_failed(
    &self,
    preset: &'static ModelPreset,
    operation_id: &str,
    code: &'static str,
  ) {
    let mut state = self.inner.state.lock().await;
    if !state
      .active
      .as_ref()
      .is_some_and(|active| active.id == operation_id)
    {
      return;
    }
    state.active = None;
    let mut view = managed_view(
      preset,
      ModelSetupPhase::Failed,
      Some(operation_id.into()),
      0,
      None,
      vec![ModelSetupAction::Retry],
    );
    view.failure = Some(ModelSetupFailureView {
      code: code.into(),
      message: "The model could not be prepared. Retry the download.".into(),
      retryable: true,
    });
    self.inner.snapshots.send_replace(Some(view));
  }

  pub(super) fn publish_progress(
    &self,
    preset: &'static ModelPreset,
    operation_id: &str,
    phase: ModelSetupPhase,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
  ) {
    let allowed_actions = if matches!(
      phase,
      ModelSetupPhase::WaitingForLock
        | ModelSetupPhase::Checking
        | ModelSetupPhase::Downloading
        | ModelSetupPhase::Verifying
    ) {
      vec![ModelSetupAction::Cancel]
    } else {
      Vec::new()
    };
    self.inner.snapshots.send_replace(Some(managed_view(
      preset,
      phase,
      Some(operation_id.into()),
      downloaded_bytes,
      total_bytes,
      allowed_actions,
    )));
  }
}
