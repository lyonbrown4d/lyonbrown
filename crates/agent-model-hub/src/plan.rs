use std::path::{Component, Path};

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

use crate::{HubModel, InstallPlan, Runtime};

/// Resolve an explicit artifact choice into a complete, revision-pinned file set.
pub fn plan(model: &HubModel, selected_file: &str, runtime: Runtime) -> Result<InstallPlan> {
  validate_revision(&model.revision)?;
  validate_repository(&model.repository)?;
  let selected = model
    .files
    .iter()
    .find(|file| file.path == selected_file)
    .ok_or_else(|| anyhow::anyhow!("selected model file is not present"))?;
  let gguf = selected.path.ends_with(".gguf");
  ensure!(
    gguf || selected.path.ends_with(".safetensors"),
    "unsupported model format"
  );
  ensure!(
    gguf || runtime == Runtime::MistralRs,
    "safetensors requires mistral.rs"
  );
  ensure!(
    !gguf || !selected.path.contains("-of-"),
    "split GGUF is not supported"
  );
  let mut files = if gguf {
    vec![selected.clone()]
  } else {
    ensure!(
      model.architecture.is_some(),
      "model architecture is unknown"
    );
    for required in ["config.json", "tokenizer.json"] {
      ensure!(
        model.files.iter().any(|file| file.path == required),
        "model configuration is incomplete"
      );
    }
    // Include every shard plus the tokenizer/configuration closure, never repository code.
    model
      .files
      .iter()
      .filter(|file| {
        file.path.ends_with(".safetensors")
          || matches!(
            file.path.as_str(),
            "config.json"
              | "tokenizer.json"
              | "tokenizer_config.json"
              | "special_tokens_map.json"
              | "generation_config.json"
              | "model.safetensors.index.json"
              | "added_tokens.json"
              | "vocab.json"
              | "merges.txt"
              | "tokenizer.model"
          )
      })
      .cloned()
      .collect()
  };
  ensure!(
    !files.is_empty() && files.len() <= 1024,
    "invalid model file count"
  );
  files.sort_by(|left, right| left.path.cmp(&right.path));
  let mut total = 0u64;
  for file in &files {
    validate_path(&file.path)?;
    ensure!(file.bytes > 0, "model file size is unknown or empty");
    if let Some(hash) = &file.sha256 {
      ensure!(
        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid model digest"
      );
    }
    total = total
      .checked_add(file.bytes)
      .ok_or_else(|| anyhow::anyhow!("model size overflow"))?;
  }
  ensure!(
    total <= 256 * 1024 * 1024 * 1024,
    "model exceeds installation limit"
  );
  let mut result = InstallPlan {
    id: String::new(),
    repository: model.repository.clone(),
    revision: model.revision.clone(),
    runtime,
    format: if gguf { "gguf" } else { "safetensors" }.into(),
    entrypoint: if gguf {
      selected.path.clone()
    } else {
      ".".into()
    },
    files,
    total_bytes: total,
    compatibility: "requires_backend_load".into(),
  };
  result.id = hex::encode(Sha256::digest(serde_json::to_vec(&result)?));
  Ok(result)
}

pub(crate) fn validate_path(value: &str) -> Result<()> {
  ensure!(
    !value.is_empty() && value.len() <= 1024 && !value.contains(['\\', ':', '\0']),
    "invalid artifact path"
  );
  ensure!(
    Path::new(value)
      .components()
      .all(|part| matches!(part, Component::Normal(_))),
    "artifact path escapes repository"
  );
  ensure!(
    !value
      .split('/')
      .any(|part| part.is_empty() || part == "." || part == ".."),
    "invalid artifact path component"
  );
  Ok(())
}

pub(crate) fn validate_repository(value: &str) -> Result<()> {
  let parts = value.split('/').collect::<Vec<_>>();
  ensure!(
    parts.len() == 2
      && parts.iter().all(|part| !part.is_empty()
        && part.len() <= 96
        && part
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        && *part != "."
        && *part != ".."),
    "invalid repository identifier"
  );
  Ok(())
}

pub(crate) fn validate_revision(value: &str) -> Result<()> {
  ensure!(
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
    "installation requires a resolved commit SHA"
  );
  Ok(())
}
