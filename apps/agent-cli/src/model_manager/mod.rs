mod catalog;
mod download;
mod progress;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use anyhow::Result;
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

pub use catalog::{default_preset, preset_by_id, select_preset};
pub use download::{
  cached_model, ensure_model, ensure_model_from_endpoint, ensure_model_from_endpoint_with_progress,
};
pub use progress::{DownloadPhase, ModelDownloadProgress};

#[derive(Debug, Clone)]
pub struct ModelPreset {
  pub id: &'static str,
  pub owner: &'static str,
  pub repository: &'static str,
  pub revision: &'static str,
  pub filename: &'static str,
  pub sha256: Cow<'static, str>,
  pub download_bytes: u64,
  pub max_bytes: u64,
  pub min_available_memory_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct PreparedModel {
  pub id: &'static str,
  pub path: PathBuf,
}

pub fn available_memory_bytes() -> u64 {
  let system = System::new_with_specifics(
    RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
  );
  system.available_memory()
}

pub fn requested_preset(requested_preset: Option<&str>) -> Result<&'static ModelPreset> {
  catalog::validate_catalog()?;
  match requested_preset {
    Some(id) => preset_by_id(id),
    None => Ok(default_preset()),
  }
}

pub async fn resolve_cached_model(
  home: &Path,
  requested_id: Option<&str>,
) -> Result<Option<PreparedModel>> {
  let available = available_memory_bytes();
  let preset = requested_preset(requested_id)?;
  if available < preset.min_available_memory_bytes {
    anyhow::bail!(
      "model preset '{}' needs at least {:.1} GiB available RAM; found {:.1} GiB",
      preset.id,
      preset.min_available_memory_bytes as f64 / (1024_u64.pow(3)) as f64,
      available as f64 / (1024_u64.pow(3)) as f64
    );
  }
  Ok(
    cached_model(&home.join(".agent").join("models"), preset)
      .await?
      .map(|path| PreparedModel {
        id: preset.id,
        path,
      }),
  )
}
