#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadPhase {
  Checking,
  Downloading,
  Verifying,
  Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDownloadProgress {
  pub phase: DownloadPhase,
  pub downloaded_bytes: u64,
  pub total_bytes: Option<u64>,
}

impl ModelDownloadProgress {
  pub fn new(phase: DownloadPhase, downloaded_bytes: u64, total_bytes: Option<u64>) -> Self {
    Self {
      phase,
      downloaded_bytes,
      total_bytes,
    }
  }

  pub fn ratio(&self) -> f64 {
    self
      .total_bytes
      .filter(|total| *total > 0)
      .map(|total| self.downloaded_bytes as f64 / total as f64)
      .unwrap_or(0.0)
      .clamp(0.0, 1.0)
  }
}
