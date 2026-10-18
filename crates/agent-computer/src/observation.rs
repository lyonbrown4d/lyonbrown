use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Bounds, DesktopError, DesktopSession, PermissionState};

/// Bounds apply before capture and again to returned bytes. No implicit truncation
/// of an image is allowed: it would change pointer-to-pixel correspondence.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ObservationLimits {
  pub max_pixels: u32,
  pub max_bytes: usize,
  pub max_windows: usize,
  pub max_nodes: usize,
  pub max_depth: usize,
}

impl Default for ObservationLimits {
  fn default() -> Self {
    Self {
      max_pixels: 16_777_216,
      max_bytes: 67_108_864,
      max_windows: 128,
      max_nodes: 128,
      max_depth: 8,
    }
  }
}

impl ObservationLimits {
  pub fn validate(&self) -> Result<(), DesktopError> {
    let ceiling = Self::default();
    if self.max_pixels == 0
      || self.max_pixels > ceiling.max_pixels
      || self.max_bytes == 0
      || self.max_bytes > ceiling.max_bytes
      || self.max_windows == 0
      || self.max_windows > ceiling.max_windows
      || self.max_nodes > ceiling.max_nodes
      || self.max_depth > ceiling.max_depth
    {
      return Err(DesktopError::InvalidRequest("observation limits".into()));
    }
    Ok(())
  }

  pub fn check_image(&self, width: u32, height: u32) -> Result<(), DesktopError> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
      || height == 0
      || pixels > u64::from(self.max_pixels)
      || pixels.saturating_mul(4) > self.max_bytes as u64
    {
      return Err(DesktopError::CapacityExceeded);
    }
    Ok(())
  }
}

/// Actual unencoded RGBA8 pixels. Debug intentionally excludes private pixels.
#[derive(Clone, Serialize, Deserialize)]
pub struct Screenshot {
  pub width: u32,
  pub height: u32,
  pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WindowObservation {
  pub id: u32,
  pub process_id: u32,
  pub title: String,
  pub bounds: Bounds,
  pub focused: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityNode {
  pub index: usize,
  pub parent: Option<usize>,
  pub role: String,
  pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessibilityObservation {
  pub state: PermissionState,
  pub nodes: Vec<AccessibilityNode>,
  pub truncated: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DesktopObservation {
  pub bounds: Bounds,
  pub screenshot: Screenshot,
  pub windows: Vec<WindowObservation>,
  pub windows_truncated: bool,
  pub accessibility: AccessibilityObservation,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DesktopSnapshot {
  pub id: Uuid,
  pub revision: u64,
  pub lease_id: Uuid,
  pub session: DesktopSession,
  pub observation: DesktopObservation,
}

impl DesktopSnapshot {
  /// Identifies bounded content independently of the observation ID/revision.
  /// A fresh capture can therefore confirm the context after model latency.
  pub fn content_digest(&self) -> Result<[u8; 32], DesktopError> {
    crate::content::content_digest(&self.session, &self.observation)
  }
}
