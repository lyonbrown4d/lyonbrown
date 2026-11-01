use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubModel {
  pub repository: String,
  pub revision: String,
  pub license: Option<String>,
  pub gated: bool,
  pub architecture: Option<String>,
  pub files: Vec<ArtifactFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactFile {
  pub path: String,
  pub bytes: u64,
  pub sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
  LlamaCpp,
  MistralRs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
  pub id: String,
  pub repository: String,
  pub revision: String,
  pub runtime: Runtime,
  pub format: String,
  pub entrypoint: String,
  pub files: Vec<ArtifactFile>,
  pub total_bytes: u64,
  /// Loading the artifact is required to establish backend compatibility.
  pub compatibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledModel {
  pub plan: InstallPlan,
  pub verified_files: Vec<ArtifactFile>,
  pub directory: PathBuf,
  pub entrypoint: PathBuf,
  pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallProgress {
  pub id: String,
  pub state: String,
  pub downloaded_bytes: u64,
  pub total_bytes: u64,
  pub current_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRecord {
  pub plan: InstallPlan,
  pub progress: InstallProgress,
}
