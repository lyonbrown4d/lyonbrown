use std::path::{Path, PathBuf};

use agent_runtime::model_manager::{
  InstallPhaseGate, ModelDownloadProgress, ModelPreset, ensure_model_with_progress_and_install_gate,
};
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait ModelInstaller: Send + Sync + 'static {
  async fn install(
    &self,
    cache_dir: &Path,
    preset: &'static ModelPreset,
    progress: watch::Sender<ModelDownloadProgress>,
    install_gate: InstallPhaseGate,
    cancellation: CancellationToken,
  ) -> Result<PathBuf>;
}

#[derive(Debug, Default)]
pub struct RuntimeModelInstaller;

#[async_trait]
impl ModelInstaller for RuntimeModelInstaller {
  async fn install(
    &self,
    cache_dir: &Path,
    preset: &'static ModelPreset,
    progress: watch::Sender<ModelDownloadProgress>,
    install_gate: InstallPhaseGate,
    cancellation: CancellationToken,
  ) -> Result<PathBuf> {
    ensure_model_with_progress_and_install_gate(
      cache_dir,
      preset,
      progress,
      cancellation,
      install_gate,
    )
    .await
  }
}
