use std::fs::File;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::storage::validate_filename;

const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(super) struct ModelLock {
  _file: File,
  destination_key: String,
}

impl ModelLock {
  pub(super) fn destination_key(&self) -> &str {
    &self.destination_key
  }
}

pub(super) async fn acquire_model_lock(
  cache_dir: &Path,
  filename: &str,
  cancellation: &CancellationToken,
) -> Result<ModelLock> {
  prepare_directory(cache_dir, "model cache directory").await?;
  let lock_dir = cache_dir.join(".locks");
  prepare_directory(&lock_dir, "model lock directory").await?;
  let destination_key = model_destination_key(filename)?;
  let lock_path = lock_dir.join(format!("{destination_key}.lock"));
  let file = tokio::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .open(&lock_path)
    .await
    .with_context(|| format!("failed to open model lock {}", lock_path.display()))?
    .into_std()
    .await;

  loop {
    if cancellation.is_cancelled() {
      bail!("model download cancelled while waiting for cache lock");
    }
    match FileExt::try_lock_exclusive(&file) {
      Ok(()) => {
        return Ok(ModelLock {
          _file: file,
          destination_key,
        });
      }
      Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
        tokio::select! {
          _ = cancellation.cancelled() => {
            bail!("model download cancelled while waiting for cache lock");
          }
          _ = tokio::time::sleep(LOCK_POLL_INTERVAL) => {}
        }
      }
      Err(error) => {
        return Err(error)
          .with_context(|| format!("failed to acquire model lock {}", lock_path.display()));
      }
    }
  }
}

/// The cache root is trusted to belong to the current user. We still reject a
/// replaced final directory or symlink before creating or opening lock files.
async fn prepare_directory(path: &Path, label: &str) -> Result<()> {
  match tokio::fs::symlink_metadata(path).await {
    Ok(metadata) => validate_directory_metadata(path, label, &metadata),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      tokio::fs::create_dir_all(path)
        .await
        .with_context(|| format!("failed to create {label} {}", path.display()))?;
      let metadata = tokio::fs::symlink_metadata(path).await?;
      validate_directory_metadata(path, label, &metadata)
    }
    Err(error) => {
      Err(error).with_context(|| format!("failed to inspect {label} {}", path.display()))
    }
  }
}

fn validate_directory_metadata(
  path: &Path,
  label: &str,
  metadata: &std::fs::Metadata,
) -> Result<()> {
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    bail!("{label} {} is not a trusted directory", path.display());
  }
  Ok(())
}

#[doc(hidden)]
pub fn model_destination_key(filename: &str) -> Result<String> {
  let identity = normalize_destination_filename(filename)?;
  Ok(format!("{:x}", Sha256::digest(identity.as_bytes())))
}

#[doc(hidden)]
pub fn normalize_destination_filename(filename: &str) -> Result<String> {
  validate_filename(filename)?;
  Ok(filename.to_lowercase())
}

pub(super) async fn cleanup_stale_staging(cache_dir: &Path, destination_key: &str) -> Result<()> {
  let prefix = format!(".download-{destination_key}-");
  let mut entries = tokio::fs::read_dir(cache_dir).await?;
  while let Some(entry) = entries.next_entry().await? {
    let name = entry.file_name();
    let Some(name) = name.to_str() else { continue };
    if !name.starts_with(&prefix) {
      continue;
    }
    let path = entry.path();
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
      tokio::fs::remove_file(&path).await?;
    } else if metadata.is_dir() {
      tokio::fs::remove_dir_all(&path).await?;
    }
  }
  Ok(())
}
