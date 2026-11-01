//! Bounded Skill file handling for an authorized daemon service.
//!
//! The caller owns authorization, network access, durable execution intent,
//! installation receipts, events and session bindings. This crate never executes
//! Skill instructions or bundled scripts and grants no capabilities.

mod archive;
mod inventory;
mod manager;
mod manifest;
mod paths;
mod references;
mod source;

pub use archive::ArchiveFormat;
pub use inventory::{FileEntry, SkillInspection};
pub use manager::{ActivatedSkill, PreparedSkill, QuarantinedSkill, SkillManager};
pub use manifest::{ParsedSkill, SkillManifest, parse_manifest};
pub use source::SkillSource;

use serde::{Deserialize, Serialize};

/// Limits are applied to actual bytes read, including compressed archive data.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Limits {
  pub download_bytes: u64,
  pub extracted_bytes: u64,
  pub file_bytes: u64,
  pub files: usize,
  pub depth: usize,
}

impl Default for Limits {
  fn default() -> Self {
    Self {
      download_bytes: 10 * 1024 * 1024,
      extracted_bytes: 25 * 1024 * 1024,
      file_bytes: 10 * 1024 * 1024,
      files: 1000,
      depth: 16,
    }
  }
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
  #[error("Skill filesystem operation failed: {0}")]
  Io(#[from] std::io::Error),
  #[error("Skill manifest is invalid: {0}")]
  Manifest(String),
  #[error("Skill package contains an unsafe or conflicting path")]
  UnsafePath,
  #[error("Skill package exceeds the configured {0} limit")]
  Limit(&'static str),
  #[error("Skill package contains unsupported filesystem entries")]
  UnsupportedEntry,
  #[error("Skill archive is invalid: {0}")]
  Archive(String),
  #[error("Skill is already installed")]
  AlreadyInstalled,
  #[error("Skill is not installed")]
  NotInstalled,
  #[error("Skill content changed; refresh the inspection before continuing")]
  ContentChanged,
  #[error("Skill source is invalid or unsupported")]
  InvalidSource,
}

pub type Result<T> = std::result::Result<T, SkillError>;
