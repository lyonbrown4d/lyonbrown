use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use hf_hub::HFClient;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{DownloadPhase, ModelDownloadProgress, ModelPreset};

pub async fn ensure_model(cache_dir: &Path, preset: &ModelPreset) -> Result<PathBuf> {
  ensure_model_from_endpoint(cache_dir, preset, "https://huggingface.co").await
}

#[doc(hidden)]
pub async fn ensure_model_from_endpoint(
  cache_dir: &Path,
  preset: &ModelPreset,
  endpoint: &str,
) -> Result<PathBuf> {
  let (progress, _observed) = watch::channel(ModelDownloadProgress::new(
    DownloadPhase::Checking,
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
  let destination = cache_dir.join(preset.filename);
  if is_valid_model(&destination, preset).await? {
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
  validate_filename(preset.filename)?;
  publish(
    &progress,
    DownloadPhase::Checking,
    0,
    Some(preset.download_bytes),
  );
  if let Some(path) = cached_model(cache_dir, preset).await? {
    let size = tokio::fs::metadata(&path).await?.len();
    publish(&progress, DownloadPhase::Ready, size, Some(size));
    return Ok(path);
  }
  tokio::fs::create_dir_all(cache_dir)
    .await
    .with_context(|| format!("failed to create model cache {}", cache_dir.display()))?;
  let destination = cache_dir.join(preset.filename);

  let staging = cache_dir.join(format!(".download-{}", Uuid::new_v4()));
  tokio::fs::create_dir(&staging).await?;
  let result = async {
    let downloaded =
      download_with_hf_hub(&staging, preset, endpoint, &progress, &cancellation).await?;
    let downloaded_size = tokio::fs::metadata(&downloaded).await?.len();
    publish(
      &progress,
      DownloadPhase::Verifying,
      downloaded_size,
      Some(downloaded_size),
    );
    if !is_valid_model(&downloaded, preset).await? {
      bail!(
        "downloaded model '{}' failed SHA-256 verification",
        preset.id
      );
    }
    install_verified(&downloaded, &destination, preset).await?;
    let size = tokio::fs::metadata(&destination).await?.len();
    publish(&progress, DownloadPhase::Ready, size, Some(size));
    Ok(destination)
  }
  .await;
  cleanup_staging(&staging, result).await
}

async fn download_with_hf_hub(
  staging: &Path,
  preset: &ModelPreset,
  endpoint: &str,
  progress: &watch::Sender<ModelDownloadProgress>,
  cancellation: &CancellationToken,
) -> Result<PathBuf> {
  let inspected = tokio::select! {
    _ = cancellation.cancelled() => bail!("model download cancelled"),
    result = inspect_file_size(endpoint, preset) => result,
  }
  .with_context(|| format!("failed to inspect model '{}'", preset.id))?;
  if let Some(size) = inspected {
    enforce_size_limit(size, preset.max_bytes)?;
  }
  let client = HFClient::builder().endpoint(endpoint).build()?;
  let repository = client.model(preset.owner, preset.repository);
  let request = repository
    .download_file_stream()
    .filename(preset.filename)
    .revision(preset.revision)
    .send();
  let (content_length, mut stream) = tokio::select! {
    _ = cancellation.cancelled() => bail!("model download cancelled"),
    result = request => result,
  }
  .with_context(|| format!("failed to download model '{}'", preset.id))?;
  if let Some(size) = content_length {
    enforce_size_limit(size, preset.max_bytes)?;
  }

  let total = content_length.or(inspected).or(Some(preset.download_bytes));
  publish(progress, DownloadPhase::Downloading, 0, total);
  let path = staging.join(preset.filename);
  let mut file = tokio::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&path)
    .await?;
  let mut downloaded = 0_u64;
  loop {
    let next = tokio::select! {
      _ = cancellation.cancelled() => bail!("model download cancelled"),
      next = stream.next() => next,
    };
    let Some(chunk) = next else { break };
    let chunk = chunk.with_context(|| format!("failed to download model '{}'", preset.id))?;
    downloaded = downloaded
      .checked_add(chunk.len() as u64)
      .context("model download size overflow")?;
    enforce_size_limit(downloaded, preset.max_bytes)?;
    file.write_all(&chunk).await?;
    publish(progress, DownloadPhase::Downloading, downloaded, total);
  }
  file.flush().await?;
  Ok(path)
}

fn publish(
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

async fn inspect_file_size(endpoint: &str, preset: &ModelPreset) -> Result<Option<u64>> {
  let mut url = reqwest::Url::parse(endpoint).context("invalid Hugging Face endpoint")?;
  let mut segments = url
    .path_segments_mut()
    .map_err(|_| anyhow::anyhow!("Hugging Face endpoint cannot be a base URL"))?;
  segments.pop_if_empty();
  segments
    .push(preset.owner)
    .push(preset.repository)
    .push("resolve")
    .push(preset.revision)
    .push(preset.filename);
  drop(segments);

  let response = reqwest::Client::builder()
    .redirect(reqwest::redirect::Policy::none())
    .build()?
    .head(url)
    .send()
    .await?
    .error_for_status()?;
  Ok(
    response
      .headers()
      .get("x-linked-size")
      .or_else(|| response.headers().get(reqwest::header::CONTENT_LENGTH))
      .and_then(|value| value.to_str().ok())
      .and_then(|value| value.parse().ok()),
  )
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
