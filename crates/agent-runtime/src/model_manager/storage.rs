use std::path::Path;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use super::ModelPreset;

pub(super) fn validate_filename(filename: &str) -> Result<()> {
  let mut components = Path::new(filename).components();
  if !matches!(components.next(), Some(std::path::Component::Normal(_)))
    || components.next().is_some()
  {
    bail!("model filename must be a single normal path component");
  }
  Ok(())
}

pub(super) fn validate_sha256(digest: &str) -> Result<()> {
  let is_lower_hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
  if digest.len() != 64 || !digest.bytes().all(is_lower_hex) {
    bail!("model SHA-256 must be lowercase hexadecimal with exactly 64 hexadecimal characters");
  }
  Ok(())
}

pub(super) async fn cleanup_staging<T>(staging: &Path, result: Result<T>) -> Result<T> {
  match tokio::fs::remove_dir_all(staging).await {
    Ok(()) => result,
    Err(cleanup) => match result {
      Ok(_) => Err(cleanup).context("failed to clean model download staging directory"),
      Err(error) => {
        Err(error.context(format!("also failed to clean staging directory: {cleanup}")))
      }
    },
  }
}

pub(super) async fn is_valid_model(
  path: &Path,
  preset: &ModelPreset,
  cancellation: Option<&CancellationToken>,
) -> Result<bool> {
  let metadata = match tokio::fs::symlink_metadata(path).await {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
    Err(error) => return Err(error.into()),
  };
  if !metadata.is_file() || metadata.file_type().is_symlink() {
    bail!("model cache entry {} is not a regular file", path.display());
  }
  if metadata.len() > preset.max_bytes {
    return Ok(false);
  }
  Ok(sha256(path, cancellation).await? == preset.sha256.as_ref())
}

/// Verify an already-installed model using the same limits and digest as the cache layer.
pub async fn verify_model_file(path: &Path, preset: &ModelPreset) -> Result<()> {
  validate_sha256(preset.sha256.as_ref())?;
  let metadata = tokio::fs::symlink_metadata(path).await?;
  if !metadata.is_file() || metadata.file_type().is_symlink() {
    bail!("model cache entry is not a regular file");
  }
  enforce_size_limit(metadata.len(), preset.max_bytes)?;
  if sha256(path, None).await? != preset.sha256.as_ref() {
    bail!("model cache entry failed SHA-256 digest verification");
  }
  Ok(())
}

pub(super) fn enforce_size_limit(size: u64, max_bytes: u64) -> Result<()> {
  if size > max_bytes {
    bail!("model download exceeds the {max_bytes} byte safety limit");
  }
  Ok(())
}

async fn sha256(path: &Path, cancellation: Option<&CancellationToken>) -> Result<String> {
  let mut file = tokio::fs::File::open(path).await?;
  let mut digest = Sha256::new();
  let mut buffer = vec![0_u8; 1024 * 1024];
  loop {
    let read = match cancellation {
      Some(cancellation) => {
        tokio::select! {
          _ = cancellation.cancelled() => bail!("model download cancelled during verification"),
          read = file.read(&mut buffer) => read?,
        }
      }
      None => file.read(&mut buffer).await?,
    };
    if read == 0 {
      break;
    }
    digest.update(&buffer[..read]);
    tokio::task::yield_now().await;
  }
  Ok(format!("{:x}", digest.finalize()))
}

pub(super) async fn install_verified(
  temporary: &Path,
  destination: &Path,
  preset: &ModelPreset,
) -> Result<()> {
  if is_valid_model(destination, preset, None).await? {
    return Ok(());
  }
  tokio::fs::OpenOptions::new()
    .write(true)
    .open(temporary)
    .await?
    .sync_all()
    .await?;
  let temporary = temporary.to_path_buf();
  let destination = destination.to_path_buf();
  let persisted_destination = destination.clone();
  tokio::task::spawn_blocking(move || {
    let temporary = tempfile::TempPath::try_from_path(temporary)?;
    temporary
      .persist(&persisted_destination)
      .map_err(|error| error.error)
  })
  .await
  .context("model install task failed")?
  .with_context(|| {
    format!(
      "failed to atomically install model cache {}",
      destination.display()
    )
  })?;
  sync_parent_directory(&destination).await
}

#[cfg(unix)]
async fn sync_parent_directory(destination: &Path) -> Result<()> {
  let parent = destination
    .parent()
    .context("model destination has no parent directory")?
    .to_path_buf();
  tokio::task::spawn_blocking(move || std::fs::File::open(parent)?.sync_all())
    .await
    .context("model cache parent sync task failed")??;
  Ok(())
}

#[cfg(not(unix))]
async fn sync_parent_directory(_destination: &Path) -> Result<()> {
  Ok(())
}
