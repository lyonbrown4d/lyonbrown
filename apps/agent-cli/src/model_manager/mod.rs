mod catalog;
mod download;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use anyhow::Result;
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

pub use catalog::{preset_by_id, select_preset};
pub use download::{ensure_model, ensure_model_from_endpoint};

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

pub fn available_memory_bytes() -> u64 {
  let system = System::new_with_specifics(
    RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
  );
  system.available_memory()
}

pub async fn resolve_automatic_model(
  home: &Path,
  requested_preset: Option<&str>,
) -> Result<(&'static str, PathBuf)> {
  catalog::validate_catalog()?;
  let available = available_memory_bytes();
  let preset = match requested_preset {
    Some(id) => preset_by_id(id)?,
    None => select_preset(available)?,
  };
  if available < preset.min_available_memory_bytes {
    anyhow::bail!(
      "model preset '{}' needs at least {:.1} GiB available RAM; found {:.1} GiB",
      preset.id,
      preset.min_available_memory_bytes as f64 / (1024_u64.pow(3)) as f64,
      available as f64 / (1024_u64.pow(3)) as f64
    );
  }
  eprintln!(
    "Selected embedded model {} for {:.1} GiB available RAM",
    preset.id,
    available as f64 / (1024_u64.pow(3)) as f64
  );
  let path = ensure_model(&home.join(".agent").join("models"), preset).await?;
  Ok((preset.id, path))
}
