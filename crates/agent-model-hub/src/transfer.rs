use std::{path::Path, time::Duration};

use anyhow::{Result, ensure};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  sync::mpsc,
};
use tokio_util::sync::CancellationToken;

use crate::files::{atomic_json, checked_parents, publish, subdir, verify};
use crate::{ArtifactFile, HubClient, InstallPlan, InstallProgress, InstallRecord};

impl HubClient {
  pub(crate) async fn transfer(
    &self,
    plan: &InstallPlan,
    stage: &Path,
    cancellation: &CancellationToken,
    progress: &Option<mpsc::Sender<InstallProgress>>,
    record: &mut InstallRecord,
  ) -> Result<()> {
    let (owner, name) = hf_hub::split_id(&plan.repository);
    let repo = self.client.model(owner, name);
    for artifact in &plan.files {
      ensure!(!cancellation.is_cancelled(), "model installation cancelled");
      crate::plan::validate_path(&artifact.path)?;
      let path = stage.join(&artifact.path);
      if let Some(parent) = path.parent() {
        checked_parents(stage, parent)?;
      }
      record.progress.current_file = artifact.path.clone();
      if artifact.sha256.is_some() && path.exists() && verify(&path, artifact).is_ok() {
        record.progress.downloaded_bytes += artifact.bytes;
        publish(progress, &record.progress);
        continue;
      }
      let partial = path.with_extension(format!(
        "{}.part",
        path
          .extension()
          .and_then(|value| value.to_str())
          .unwrap_or("file")
      ));
      let transfer = async {
        let (mut received, mut digest) = prefix(&partial, artifact).await?;
        let offset = received;
        let mut file = tokio::fs::OpenOptions::new()
          .create(true)
          .write(true)
          .append(offset > 0)
          .truncate(offset == 0)
          .open(&partial)
          .await?;
        ensure!(
          file.metadata().await?.len() == offset,
          "partial model file changed"
        );
        if offset < artifact.bytes {
          let (length, mut stream) = repo
            .download_file_stream()
            .filename(&artifact.path)
            .revision(&plan.revision)
            .maybe_range((offset > 0).then_some(offset..artifact.bytes))
            .send()
            .await?;
          let remaining = artifact.bytes - offset;
          ensure!(
            if offset > 0 {
              length == Some(remaining)
            } else {
              length.is_none_or(|bytes| bytes == remaining)
            },
            "model range length changed or server ignored resume"
          );
          while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            received = received
              .checked_add(chunk.len() as u64)
              .ok_or_else(|| anyhow::anyhow!("model size overflow"))?;
            ensure!(
              received <= artifact.bytes,
              "model file exceeds planned size"
            );
            digest.update(&chunk);
            file.write_all(&chunk).await?;
            let mut update = record.progress.clone();
            update.downloaded_bytes += received;
            publish(progress, &update);
          }
        }
        ensure!(received == artifact.bytes, "incomplete model file");
        if let Some(expected) = &artifact.sha256
          && !hex::encode(digest.finalize()).eq_ignore_ascii_case(expected)
        {
          // Invalid cached bytes cannot be reused by the next explicit retry.
          file.set_len(0).await?;
          anyhow::bail!("model digest mismatch");
        }
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&partial, &path).await?;
        Ok::<_, anyhow::Error>(())
      };
      tokio::select! { biased; _ = cancellation.cancelled() => anyhow::bail!("model installation cancelled"),
      result = tokio::time::timeout(Duration::from_secs(3600), transfer) => result??, }
      record.progress.downloaded_bytes += artifact.bytes;
      let jobs = subdir(&self.root, "jobs")?;
      atomic_json(&jobs.join(format!("{}.json", plan.id)), record)?;
    }
    Ok(())
  }
}

/// Prefix bytes are only reusable with the pinned full-file digest checked before activation.
async fn prefix(path: &Path, artifact: &ArtifactFile) -> Result<(u64, Sha256)> {
  let metadata = match tokio::fs::symlink_metadata(path).await {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, Sha256::new())),
    Err(error) => return Err(error.into()),
  };
  ensure!(
    metadata.is_file() && !metadata.file_type().is_symlink(),
    "invalid partial file"
  );
  ensure!(
    metadata.len() <= artifact.bytes,
    "partial file exceeds model size"
  );
  if artifact.sha256.is_none() {
    return Ok((0, Sha256::new()));
  }
  let mut file = tokio::fs::File::open(path).await?;
  let mut digest = Sha256::new();
  let mut buffer = vec![0; 64 * 1024];
  let mut bytes = 0;
  loop {
    let count = file.read(&mut buffer).await?;
    if count == 0 {
      break;
    }
    bytes += count as u64;
    ensure!(bytes <= artifact.bytes, "partial file exceeds model size");
    digest.update(&buffer[..count]);
  }
  ensure!(
    bytes == metadata.len(),
    "partial model changed during inspection"
  );
  Ok((bytes, digest))
}
