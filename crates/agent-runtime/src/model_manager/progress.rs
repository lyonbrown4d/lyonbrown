#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadPhase {
  WaitingForLock,
  Checking,
  Downloading,
  Verifying,
  Installing,
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

/// A linearization gate immediately before an installed model replaces its destination.
#[doc(hidden)]
#[derive(Clone)]
pub struct InstallPhaseGate {
  requests: mpsc::Sender<oneshot::Sender<bool>>,
}

#[doc(hidden)]
pub fn install_phase_gate() -> (InstallPhaseGate, mpsc::Receiver<oneshot::Sender<bool>>) {
  let (requests, receiver) = mpsc::channel(1);
  (InstallPhaseGate { requests }, receiver)
}

impl InstallPhaseGate {
  #[doc(hidden)]
  pub async fn wait(&self, cancellation: &CancellationToken) -> Result<()> {
    let (permit, decision) = oneshot::channel();
    self
      .requests
      .send(permit)
      .await
      .map_err(|_| anyhow::anyhow!("model install phase gate closed"))?;
    let allowed = tokio::select! {
      _ = cancellation.cancelled() => false,
      decision = decision => decision.unwrap_or(false),
    };
    if !allowed {
      bail!("model installation cancelled before the install phase");
    }
    Ok(())
  }
}
use anyhow::{Result, bail};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
