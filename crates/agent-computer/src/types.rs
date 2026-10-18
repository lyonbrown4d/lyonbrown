use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct DesktopIdentity {
  pub node_id: String,
  /// OS graphical session identity, obtained by the authenticated helper.
  pub user_session: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DesktopSession {
  pub id: Uuid,
  pub identity: DesktopIdentity,
  pub scope: TargetScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TargetScope {
  Display(u32),
  Window(u32),
}

/// Read-only native target metadata. Labels can contain private desktop content.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct DesktopTarget {
  pub scope: TargetScope,
  pub label: String,
  pub process_id: Option<u32>,
  pub bounds: Bounds,
}

impl std::fmt::Debug for DesktopTarget {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("DesktopTarget")
      .field("scope", &self.scope)
      .field("process_id", &self.process_id)
      .field("bounds", &self.bounds)
      .finish_non_exhaustive()
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

impl Bounds {
  pub fn contains(&self, x: i32, y: i32) -> bool {
    i64::from(x) >= i64::from(self.x)
      && i64::from(y) >= i64::from(self.y)
      && i64::from(x) < i64::from(self.x) + i64::from(self.width)
      && i64::from(y) < i64::from(self.y) + i64::from(self.height)
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PermissionState {
  Granted,
  Denied,
  Unsupported,
  Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DesktopCapabilities {
  pub identity: DesktopIdentity,
  pub interactive: bool,
  pub capture: PermissionState,
  pub input: PermissionState,
  pub accessibility: PermissionState,
}

impl DesktopCapabilities {
  pub fn can_control(&self) -> bool {
    self.interactive
      && self.capture == PermissionState::Granted
      && self.input == PermissionState::Granted
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Error, Serialize, Deserialize)]
pub enum DesktopError {
  #[error("desktop session is locked")]
  Locked,
  #[error("interactive graphical session is unavailable")]
  SessionUnavailable,
  #[error("desktop lock state cannot be established safely")]
  SessionStateUnknown,
  #[error("permission denied: {0}")]
  PermissionDenied(String),
  #[error("unsupported desktop capability: {0}")]
  Unsupported(String),
  #[error("desktop backend unavailable: {0}")]
  BackendUnavailable(String),
  #[error("desktop lease is already held")]
  LeaseBusy,
  #[error("desktop lease is expired or revoked")]
  LeaseInvalid,
  #[error("snapshot is stale or does not match the lease")]
  StaleSnapshot,
  #[error("action identity was already used; automatic replay is forbidden")]
  DuplicateAction,
  #[error("operation cancelled")]
  Cancelled,
  #[error("operation deadline exceeded")]
  DeadlineExceeded,
  #[error("invalid desktop request: {0}")]
  InvalidRequest(String),
  #[error("bounded desktop capacity exceeded")]
  CapacityExceeded,
  #[error("desktop content digest is unavailable")]
  ContentDigestUnavailable,
  #[error("desktop coordination state is unavailable")]
  CoordinationUnavailable,
}
