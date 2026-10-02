use std::collections::HashSet;

use anyhow::{Result, bail};

use super::ModelPreset;
use super::locking::normalize_destination_filename;

const GIB: u64 = 1024 * 1024 * 1024;

pub(super) const PRESETS: &[ModelPreset] = &[
  ModelPreset {
    id: "qwen3-0.6b-q8_0",
    owner: "Qwen",
    repository: "Qwen3-0.6B-GGUF",
    revision: "main",
    filename: "Qwen3-0.6B-Q8_0.gguf",
    sha256: std::borrow::Cow::Borrowed(
      "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
    ),
    download_bytes: 639_000_000,
    max_bytes: GIB,
    min_available_memory_bytes: 2 * GIB,
  },
  ModelPreset {
    id: "qwen3-1.7b-q4_k_m",
    owner: "Qwen",
    repository: "Qwen3-1.7B-GGUF",
    revision: "main",
    filename: "Qwen3-1.7B-Q4_K_M.gguf",
    sha256: std::borrow::Cow::Borrowed(
      "228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f",
    ),
    download_bytes: 1_110_000_000,
    max_bytes: 2 * GIB,
    min_available_memory_bytes: 4 * GIB,
  },
  ModelPreset {
    id: "qwen3-4b-q4_k_m",
    owner: "Qwen",
    repository: "Qwen3-4B-GGUF",
    revision: "main",
    filename: "Qwen3-4B-Q4_K_M.gguf",
    sha256: std::borrow::Cow::Borrowed(
      "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5",
    ),
    download_bytes: 2_500_000_000,
    max_bytes: 4 * GIB,
    min_available_memory_bytes: 8 * GIB,
  },
  ModelPreset {
    id: "qwen3-8b-q4_k_m",
    owner: "Qwen",
    repository: "Qwen3-8B-GGUF",
    revision: "main",
    filename: "Qwen3-8B-Q4_K_M.gguf",
    sha256: std::borrow::Cow::Borrowed(
      "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
    ),
    download_bytes: 5_030_000_000,
    max_bytes: 7 * GIB,
    min_available_memory_bytes: 14 * GIB,
  },
];

pub fn select_preset(available_memory_bytes: u64) -> Result<&'static ModelPreset> {
  PRESETS
    .iter()
    .rev()
    .find(|preset| available_memory_bytes >= preset.min_available_memory_bytes)
    .ok_or_else(|| {
      anyhow::anyhow!(
        "embedded llama.cpp needs at least 2 GiB of currently available RAM; found {:.1} GiB",
        available_memory_bytes as f64 / GIB as f64
      )
    })
}

pub fn default_preset() -> &'static ModelPreset {
  &PRESETS[1]
}

pub fn preset_by_id(id: &str) -> Result<&'static ModelPreset> {
  PRESETS
    .iter()
    .find(|preset| preset.id == id)
    .ok_or_else(|| anyhow::anyhow!("unknown embedded model preset '{id}'"))
}

pub fn validate_catalog() -> Result<()> {
  validate_catalog_entries(PRESETS)
}

#[doc(hidden)]
pub fn validate_catalog_entries(presets: &[ModelPreset]) -> Result<()> {
  for pair in presets.windows(2) {
    if pair[0].min_available_memory_bytes >= pair[1].min_available_memory_bytes {
      bail!("embedded model catalog memory thresholds must be increasing");
    }
  }
  let mut filenames = HashSet::new();
  for preset in presets {
    if !filenames.insert(normalize_destination_filename(preset.filename)?) {
      bail!(
        "embedded model catalog has a case-insensitive filename conflict for '{}'",
        preset.filename
      );
    }
  }
  Ok(())
}
