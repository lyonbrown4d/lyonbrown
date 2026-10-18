use agent_config::{LocalModelBackend, LocalModelFormat, LocalModelSettings};
use agent_inference::InferenceBackend;
use anyhow::{Result, ensure};
use std::{io::Read, path::Path, sync::Arc};

pub(super) fn validate(profile: &LocalModelSettings) -> Result<()> {
  profile.validate()?;
  match profile.backend {
    LocalModelBackend::Llama => ensure!(
      cfg!(feature = "local-llama"),
      "local llama backend is unavailable; build with local-llama"
    ),
    LocalModelBackend::Mistral => ensure!(
      cfg!(feature = "local-mistral"),
      "local mistral backend is unavailable; build with local-mistral"
    ),
  }
  let metadata = std::fs::symlink_metadata(&profile.path)?;
  ensure!(
    !metadata.file_type().is_symlink(),
    "local model path must not be a symlink"
  );
  match profile.format {
    LocalModelFormat::Gguf => {
      ensure!(
        metadata.is_file() && metadata.len() <= profile.max_model_bytes,
        "local GGUF must be a regular file within max_model_bytes"
      );
      let mut magic = [0; 4];
      std::fs::File::open(&profile.path)?.read_exact(&mut magic)?;
      ensure!(&magic == b"GGUF", "local model file is not GGUF");
    }
    LocalModelFormat::Directory => {
      ensure!(metadata.is_dir(), "local model path must be a directory");
      let mut bytes = 0;
      let mut files = 0;
      directory_budget(
        &profile.path,
        profile.max_model_bytes,
        &mut bytes,
        &mut files,
        0,
      )?;
      ensure!(
        profile.path.join("config.json").is_file() && profile.path.join("tokenizer.json").is_file(),
        "local directory requires config.json and tokenizer.json"
      );
      let entries = std::fs::read_dir(&profile.path)?.collect::<std::io::Result<Vec<_>>>()?;
      ensure!(
        entries.iter().any(|entry| entry
          .path()
          .extension()
          .is_some_and(|extension| extension == "safetensors")),
        "local directory requires existing safetensors weights"
      );
    }
  }
  Ok(())
}
fn directory_budget(
  path: &Path,
  maximum: u64,
  bytes: &mut u64,
  files: &mut usize,
  depth: usize,
) -> Result<()> {
  ensure!(
    depth <= 16,
    "local model directory nesting exceeds 16 levels"
  );
  for entry in std::fs::read_dir(path)? {
    let entry = entry?;
    let metadata = std::fs::symlink_metadata(entry.path())?;
    *files += 1;
    ensure!(*files <= 4096, "local model directory exceeds 4096 entries");
    ensure!(
      !metadata.file_type().is_symlink(),
      "local model directory must not contain symlinks"
    );
    if metadata.is_dir() {
      directory_budget(&entry.path(), maximum, bytes, files, depth + 1)?;
    } else {
      ensure!(
        metadata.is_file(),
        "local model contains a non-regular file"
      );
      *bytes = bytes
        .checked_add(metadata.len())
        .ok_or_else(|| anyhow::anyhow!("local model size overflow"))?;
      ensure!(*bytes <= maximum, "local model exceeds max_model_bytes");
    }
  }
  Ok(())
}
pub(super) async fn backend(profile: &LocalModelSettings) -> Result<Arc<dyn InferenceBackend>> {
  match profile.backend {
    LocalModelBackend::Llama => llama(profile).await,
    LocalModelBackend::Mistral => mistral(profile).await,
  }
}
#[cfg(feature = "local-llama")]
async fn llama(profile: &LocalModelSettings) -> Result<Arc<dyn InferenceBackend>> {
  Ok(Arc::new(
    agent_inference::LlamaCppBackend::load(
      profile.id.clone(),
      &profile.path,
      profile.context_window,
      0,
    )
    .await?,
  ))
}
#[cfg(not(feature = "local-llama"))]
async fn llama(_: &LocalModelSettings) -> Result<Arc<dyn InferenceBackend>> {
  anyhow::bail!("local llama backend is unavailable; build with local-llama")
}
#[cfg(feature = "local-mistral")]
async fn mistral(profile: &LocalModelSettings) -> Result<Arc<dyn InferenceBackend>> {
  // Native tool/image support must have model-specific acceptance before exposing it.
  let backend = match profile.format {
    LocalModelFormat::Gguf => {
      agent_inference::MistralBackend::load_gguf(
        profile.id.clone(),
        &profile.path,
        profile.context_window,
        false,
        true,
      )
      .await?
    }
    LocalModelFormat::Directory => {
      agent_inference::MistralBackend::load(
        profile.id.clone(),
        &profile.path,
        profile.context_window,
        false,
        false,
        true,
      )
      .await?
    }
  };
  Ok(Arc::new(backend))
}
#[cfg(not(feature = "local-mistral"))]
async fn mistral(_: &LocalModelSettings) -> Result<Arc<dyn InferenceBackend>> {
  anyhow::bail!("local mistral backend is unavailable; build with local-mistral")
}
