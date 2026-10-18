use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelBackend {
  Llama,
  Mistral,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelFormat {
  #[default]
  Gguf,
  Directory,
}
/// Explicit local files only. Loading never authorizes a download or GPU offload.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalModelSettings {
  pub id: String,
  pub backend: LocalModelBackend,
  pub format: LocalModelFormat,
  pub path: PathBuf,
  pub context_window: u32,
  pub max_output_tokens: u32,
  pub parallel_requests: usize,
  /// Upper bound on the input files; this is not an estimate of runtime RAM.
  pub max_model_bytes: u64,
}
impl Default for LocalModelSettings {
  fn default() -> Self {
    Self {
      id: String::new(),
      backend: LocalModelBackend::Llama,
      format: LocalModelFormat::Gguf,
      path: PathBuf::new(),
      context_window: 4096,
      max_output_tokens: 512,
      parallel_requests: 1,
      max_model_bytes: 8 * 1024 * 1024 * 1024,
    }
  }
}
impl LocalModelSettings {
  pub fn validate(&self) -> Result<()> {
    ensure!(
      !self.id.is_empty()
        && self.id.len() <= 128
        && self
          .id
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || b"_-./".contains(&byte)),
      "local model id must be 1..128 ASCII identifier characters"
    );
    ensure!(self.path.is_absolute(), "local model path must be absolute");
    ensure!(
      (512..=262_144).contains(&self.context_window),
      "local model context_window must be between 512 and 262144"
    );
    ensure!(
      self.max_output_tokens > 0 && self.max_output_tokens < self.context_window,
      "local model max_output_tokens must be positive and smaller than context_window"
    );
    ensure!(
      (1..=16).contains(&self.parallel_requests),
      "local model parallel_requests must be between 1 and 16"
    );
    ensure!(
      self.max_model_bytes > 0,
      "local model max_model_bytes must be positive"
    );
    ensure!(
      self.backend != LocalModelBackend::Llama || self.format == LocalModelFormat::Gguf,
      "llama backend requires GGUF format"
    );
    Ok(())
  }
}
pub fn validate_local_models(models: &[LocalModelSettings]) -> Result<()> {
  ensure!(
    models.len() <= 16,
    "at most 16 local models may be configured"
  );
  let mut ids = BTreeSet::new();
  for model in models {
    model.validate()?;
    ensure!(ids.insert(&model.id), "local model ids must be unique");
  }
  Ok(())
}
