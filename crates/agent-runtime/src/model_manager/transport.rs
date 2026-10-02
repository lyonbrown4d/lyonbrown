use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use hf_hub::HFClient;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::download::publish;
use super::storage::enforce_size_limit;
use super::{DownloadPhase, ModelDownloadProgress, ModelPreset};

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct ModelDownloadTimeouts {
  pub connect: Duration,
  pub request: Duration,
  pub idle: Duration,
}

impl Default for ModelDownloadTimeouts {
  fn default() -> Self {
    Self {
      connect: Duration::from_secs(10),
      request: Duration::from_secs(30),
      idle: Duration::from_secs(30),
    }
  }
}

pub(super) async fn download_with_hf_hub(
  staging: &Path,
  preset: &ModelPreset,
  endpoint: &str,
  progress: &watch::Sender<ModelDownloadProgress>,
  cancellation: &CancellationToken,
  timeouts: ModelDownloadTimeouts,
) -> Result<PathBuf> {
  let http_client = reqwest::Client::builder()
    .connect_timeout(timeouts.connect)
    .redirect(reqwest::redirect::Policy::limited(10))
    .build()?;
  let inspected = tokio::select! {
    _ = cancellation.cancelled() => bail!("model download cancelled"),
    result = tokio::time::timeout(
      timeouts.request,
      inspect_file_size(&http_client, endpoint, preset),
    ) => match result {
      Ok(result) => result,
      Err(_) => bail!("model metadata request timed out"),
    },
  }
  .with_context(|| format!("failed to inspect model '{}'", preset.id))?;
  if let Some(size) = inspected {
    enforce_size_limit(size, preset.max_bytes)?;
  }
  let client = HFClient::builder()
    .endpoint(endpoint)
    .client(http_client)
    .build()?;
  let repository = client.model(preset.owner, preset.repository);
  let request = repository
    .download_file_stream()
    .filename(preset.filename)
    .revision(preset.revision)
    .send();
  let (content_length, mut stream) = tokio::select! {
    _ = cancellation.cancelled() => bail!("model download cancelled"),
    result = tokio::time::timeout(timeouts.request, request) => match result {
      Ok(result) => result,
      Err(_) => bail!("model download request timed out"),
    },
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
      next = tokio::time::timeout(timeouts.idle, stream.next()) => match next {
        Ok(next) => next,
        Err(_) => bail!("model download stream timed out waiting for data"),
      },
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
  file.sync_all().await?;
  Ok(path)
}

async fn inspect_file_size(
  client: &reqwest::Client,
  endpoint: &str,
  preset: &ModelPreset,
) -> Result<Option<u64>> {
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

  let response = client.head(url).send().await?.error_for_status()?;
  Ok(
    response
      .headers()
      .get("x-linked-size")
      .or_else(|| response.headers().get(reqwest::header::CONTENT_LENGTH))
      .and_then(|value| value.to_str().ok())
      .and_then(|value| value.parse().ok()),
  )
}
