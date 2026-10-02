use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::locking::{acquire_model_lock, cleanup_stale_staging};
use super::storage::{
  cleanup_staging, install_verified, is_valid_model, validate_filename, validate_sha256,
};
use super::transport::{ModelDownloadTimeouts, download_with_hf_hub};
use super::{DownloadPhase, InstallPhaseGate, ModelDownloadProgress, ModelPreset};

pub async fn ensure_model(cache_dir: &Path, preset: &ModelPreset) -> Result<PathBuf> {
  let (progress, _observed) = watch::channel(ModelDownloadProgress::new(
    DownloadPhase::WaitingForLock,
    0,
    Some(preset.download_bytes),
  ));
  ensure_model_with_progress(cache_dir, preset, progress, CancellationToken::new()).await
}

pub async fn ensure_model_with_progress(
  cache_dir: &Path,
  preset: &ModelPreset,
  progress: watch::Sender<ModelDownloadProgress>,
  cancellation: CancellationToken,
) -> Result<PathBuf> {
  ensure_model_with_optional_install_gate(cache_dir, preset, progress, cancellation, None).await
}

#[doc(hidden)]
pub async fn ensure_model_with_progress_and_install_gate(
  cache_dir: &Path,
  preset: &ModelPreset,
  progress: watch::Sender<ModelDownloadProgress>,
  cancellation: CancellationToken,
  install_gate: InstallPhaseGate,
) -> Result<PathBuf> {
  ensure_model_with_optional_install_gate(
    cache_dir,
    preset,
    progress,
    cancellation,
    Some(install_gate),
  )
  .await
}

async fn ensure_model_with_optional_install_gate(
  cache_dir: &Path,
  preset: &ModelPreset,
  progress: watch::Sender<ModelDownloadProgress>,
  cancellation: CancellationToken,
  install_gate: Option<InstallPhaseGate>,
) -> Result<PathBuf> {
  ensure_model_from_endpoint_with_progress_and_timeouts_internal(
    cache_dir,
    preset,
    "https://huggingface.co",
    progress,
    cancellation,
    ModelDownloadTimeouts::default(),
    install_gate,
  )
  .await
}

#[doc(hidden)]
pub async fn ensure_model_from_endpoint(
  cache_dir: &Path,
  preset: &ModelPreset,
  endpoint: &str,
) -> Result<PathBuf> {
  let (progress, _observed) = watch::channel(ModelDownloadProgress::new(
    DownloadPhase::WaitingForLock,
    0,
    Some(preset.download_bytes),
  ));
  ensure_model_from_endpoint_with_progress(
    cache_dir,
    preset,
    endpoint,
    progress,
    CancellationToken::new(),
  )
  .await
}

pub async fn cached_model(cache_dir: &Path, preset: &ModelPreset) -> Result<Option<PathBuf>> {
  validate_filename(preset.filename)?;
  validate_sha256(preset.sha256.as_ref())?;
  let destination = cache_dir.join(preset.filename);
  if is_valid_model(&destination, preset, None).await? {
    Ok(Some(destination))
  } else {
    Ok(None)
  }
}

#[doc(hidden)]
pub async fn ensure_model_from_endpoint_with_progress(
  cache_dir: &Path,
  preset: &ModelPreset,
  endpoint: &str,
  progress: watch::Sender<ModelDownloadProgress>,
  cancellation: CancellationToken,
) -> Result<PathBuf> {
  ensure_model_from_endpoint_with_progress_and_timeouts(
    cache_dir,
    preset,
    endpoint,
    progress,
    cancellation,
    ModelDownloadTimeouts::default(),
  )
  .await
}

#[doc(hidden)]
pub async fn ensure_model_from_endpoint_with_progress_and_timeouts(
  cache_dir: &Path,
  preset: &ModelPreset,
  endpoint: &str,
  progress: watch::Sender<ModelDownloadProgress>,
  cancellation: CancellationToken,
  timeouts: ModelDownloadTimeouts,
) -> Result<PathBuf> {
  ensure_model_from_endpoint_with_progress_and_timeouts_internal(
    cache_dir,
    preset,
    endpoint,
    progress,
    cancellation,
    timeouts,
    None,
  )
  .await
}

async fn ensure_model_from_endpoint_with_progress_and_timeouts_internal(
  cache_dir: &Path,
  preset: &ModelPreset,
  endpoint: &str,
  progress: watch::Sender<ModelDownloadProgress>,
  cancellation: CancellationToken,
  timeouts: ModelDownloadTimeouts,
  install_gate: Option<InstallPhaseGate>,
) -> Result<PathBuf> {
  validate_filename(preset.filename)?;
  validate_sha256(preset.sha256.as_ref())?;
  publish_phase(
    &progress,
    DownloadPhase::WaitingForLock,
    0,
    Some(preset.download_bytes),
  )
  .await;
  let lock = acquire_model_lock(cache_dir, preset.filename, &cancellation).await?;
  cleanup_stale_staging(cache_dir, lock.destination_key()).await?;
  publish_phase(
    &progress,
    DownloadPhase::Checking,
    0,
    Some(preset.download_bytes),
  )
  .await;
  if let Some(path) = cached_model(cache_dir, preset).await? {
    let size = tokio::fs::metadata(&path).await?.len();
    publish(&progress, DownloadPhase::Ready, size, Some(size));
    return Ok(path);
  }
  let destination = cache_dir.join(preset.filename);

  let staging = cache_dir.join(format!(
    ".download-{}-{}",
    lock.destination_key(),
    Uuid::new_v4()
  ));
  tokio::fs::create_dir(&staging).await?;
  let result = download_verify_install(
    DownloadTarget {
      staging: &staging,
      destination: &destination,
      preset,
      endpoint,
    },
    &progress,
    &cancellation,
    timeouts,
    install_gate.as_ref(),
  )
  .await;
  finalize_download_after_cleanup(&staging, result, &progress).await
}

struct DownloadTarget<'a> {
  staging: &'a Path,
  destination: &'a Path,
  preset: &'a ModelPreset,
  endpoint: &'a str,
}

async fn download_verify_install(
  target: DownloadTarget<'_>,
  progress: &watch::Sender<ModelDownloadProgress>,
  cancellation: &CancellationToken,
  timeouts: ModelDownloadTimeouts,
  install_gate: Option<&InstallPhaseGate>,
) -> Result<PathBuf> {
  let downloaded = download_with_hf_hub(
    target.staging,
    target.preset,
    target.endpoint,
    progress,
    cancellation,
    timeouts,
  )
  .await?;
  let downloaded_size = tokio::fs::metadata(&downloaded).await?.len();
  publish_phase(
    progress,
    DownloadPhase::Verifying,
    downloaded_size,
    Some(downloaded_size),
  )
  .await;
  if !is_valid_model(&downloaded, target.preset, Some(cancellation)).await? {
    bail!(
      "downloaded model '{}' failed SHA-256 verification",
      target.preset.id
    );
  }
  publish_phase(
    progress,
    DownloadPhase::Installing,
    downloaded_size,
    Some(downloaded_size),
  )
  .await;
  if let Some(gate) = install_gate {
    gate.wait(cancellation).await?;
  }
  install_verified(&downloaded, target.destination, target.preset).await?;
  Ok(target.destination.to_path_buf())
}

#[doc(hidden)]
pub async fn finalize_download_after_cleanup(
  staging: &Path,
  result: Result<PathBuf>,
  progress: &watch::Sender<ModelDownloadProgress>,
) -> Result<PathBuf> {
  let destination = cleanup_staging(staging, result).await?;
  let size = tokio::fs::metadata(&destination).await?.len();
  publish(progress, DownloadPhase::Ready, size, Some(size));
  Ok(destination)
}

pub(super) fn publish(
  progress: &watch::Sender<ModelDownloadProgress>,
  phase: DownloadPhase,
  downloaded_bytes: u64,
  total_bytes: Option<u64>,
) {
  progress.send_replace(ModelDownloadProgress::new(
    phase,
    downloaded_bytes,
    total_bytes,
  ));
}

async fn publish_phase(
  progress: &watch::Sender<ModelDownloadProgress>,
  phase: DownloadPhase,
  downloaded_bytes: u64,
  total_bytes: Option<u64>,
) {
  publish(progress, phase, downloaded_bytes, total_bytes);
  tokio::task::yield_now().await;
}
