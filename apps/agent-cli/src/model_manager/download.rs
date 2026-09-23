use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use hf_hub::HFClient;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use super::ModelPreset;

pub async fn ensure_model(cache_dir: &Path, preset: &ModelPreset) -> Result<PathBuf> {
  ensure_model_from_endpoint(cache_dir, preset, "https://huggingface.co").await
}

#[doc(hidden)]
pub async fn ensure_model_from_endpoint(
  cache_dir: &Path,
  preset: &ModelPreset,
  endpoint: &str,
) -> Result<PathBuf> {
  validate_filename(preset.filename)?;
  tokio::fs::create_dir_all(cache_dir)
    .await
    .with_context(|| format!("failed to create model cache {}", cache_dir.display()))?;
  let destination = cache_dir.join(preset.filename);
  if is_valid_model(&destination, preset).await? {
    return Ok(destination);
  }

  let staging = cache_dir.join(format!(".download-{}", Uuid::new_v4()));
  tokio::fs::create_dir(&staging).await?;
  let result = async {
    let downloaded = download_with_hf_hub(&staging, preset, endpoint).await?;
    if !is_valid_model(&downloaded, preset).await? {
      bail!(
        "downloaded model '{}' failed SHA-256 verification",
        preset.id
      );
    }
    install_verified(&downloaded, &destination, preset).await?;
    Ok(destination)
  }
  .await;
  cleanup_staging(&staging, result).await
}

async fn download_with_hf_hub(
  staging: &Path,
  preset: &ModelPreset,
  endpoint: &str,
) -> Result<PathBuf> {
  eprintln!(
    "Downloading {} ({:.2} GiB) from Hugging Face...",
    preset.id,
    preset.download_bytes as f64 / (1024_u64.pow(3)) as f64
  );
  let client = HFClient::builder().endpoint(endpoint).build()?;
  let repository = client.model(preset.owner, preset.repository);
  let metadata = repository
    .get_file_metadata()
    .filepath(preset.filename)
    .revision(preset.revision)
    .send()
    .await
    .with_context(|| format!("failed to inspect model '{}'", preset.id))?;
  enforce_size_limit(metadata.file_size, preset.max_bytes)?;
  let (content_length, mut stream) = repository
    .download_file_stream()
    .filename(preset.filename)
    .revision(preset.revision)
    .send()
    .await
    .with_context(|| format!("failed to download model '{}'", preset.id))?;
  if let Some(size) = content_length {
    enforce_size_limit(size, preset.max_bytes)?;
  }

  let path = staging.join(preset.filename);
  let mut file = tokio::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&path)
    .await?;
  let mut downloaded = 0_u64;
  while let Some(chunk) = stream.next().await {
    let chunk = chunk.with_context(|| format!("failed to download model '{}'", preset.id))?;
    downloaded = downloaded
      .checked_add(chunk.len() as u64)
      .context("model download size overflow")?;
    enforce_size_limit(downloaded, preset.max_bytes)?;
    file.write_all(&chunk).await?;
  }
  file.flush().await?;
  Ok(path)
}

fn enforce_size_limit(size: u64, max_bytes: u64) -> Result<()> {
  if size > max_bytes {
    bail!("model download exceeds the {max_bytes} byte safety limit");
  }
  Ok(())
}

fn validate_filename(filename: &str) -> Result<()> {
  let mut components = Path::new(filename).components();
  if !matches!(components.next(), Some(std::path::Component::Normal(_)))
    || components.next().is_some()
  {
    bail!("model filename must be a single normal path component");
  }
  Ok(())
}

async fn cleanup_staging<T>(staging: &Path, result: Result<T>) -> Result<T> {
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

async fn is_valid_model(path: &Path, preset: &ModelPreset) -> Result<bool> {
  let metadata = match tokio::fs::symlink_metadata(path).await {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
    Err(error) => return Err(error.into()),
  };
  if !metadata.is_file() || metadata.file_type().is_symlink() {
    bail!("model cache entry {} is not a regular file", path.display());
  }
  enforce_size_limit(metadata.len(), preset.max_bytes)?;
  Ok(sha256(path).await? == preset.sha256.as_ref())
}

async fn sha256(path: &Path) -> Result<String> {
  let mut file = tokio::fs::File::open(path).await?;
  let mut digest = Sha256::new();
  let mut buffer = vec![0_u8; 1024 * 1024];
  loop {
    let read = file.read(&mut buffer).await?;
    if read == 0 {
      break;
    }
    digest.update(&buffer[..read]);
  }
  Ok(format!("{:x}", digest.finalize()))
}

async fn install_verified(
  temporary: &Path,
  destination: &Path,
  preset: &ModelPreset,
) -> Result<()> {
  if is_valid_model(destination, preset).await? {
    return Ok(());
  }
  let install = destination.with_extension(format!("gguf.{}.part", Uuid::new_v4()));
  tokio::fs::rename(temporary, &install).await?;
  let result = async {
    if tokio::fs::try_exists(destination).await? {
      tokio::fs::remove_file(destination).await?;
    }
    tokio::fs::rename(&install, destination)
      .await
      .with_context(|| format!("failed to install model cache {}", destination.display()))
  }
  .await;
  if result.is_err() {
    let _ = tokio::fs::remove_file(&install).await;
  }
  result
}
