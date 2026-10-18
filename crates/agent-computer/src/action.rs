use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{DesktopError, LeaseToken};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MouseButton {
  Left,
  Middle,
  Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ScrollAxis {
  Vertical,
  Horizontal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DesktopKey {
  Enter,
  Tab,
  Escape,
  Space,
  Backspace,
  Delete,
  Up,
  Down,
  Left,
  Right,
  Home,
  End,
  PageUp,
  PageDown,
}

/// A single bounded input step. Text is private data and is not Debug-formatted.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum DesktopAction {
  Move {
    x: i32,
    y: i32,
  },
  Click {
    x: i32,
    y: i32,
    button: MouseButton,
  },
  Scroll {
    x: i32,
    y: i32,
    axis: ScrollAxis,
    lines: i32,
  },
  Key {
    key: DesktopKey,
  },
  Text {
    text: String,
  },
}

impl DesktopAction {
  pub fn validate(&self) -> Result<(), DesktopError> {
    match self {
      Self::Text { text } if text.is_empty() || text.len() > 4096 => Err(
        DesktopError::InvalidRequest("text must contain 1..=4096 bytes".into()),
      ),
      Self::Scroll { lines, .. } if *lines == 0 || lines.unsigned_abs() > 100 => Err(
        DesktopError::InvalidRequest("scroll must contain 1..=100 lines".into()),
      ),
      _ => Ok(()),
    }
  }

  pub fn point(&self) -> Option<(i32, i32)> {
    match self {
      Self::Move { x, y } | Self::Click { x, y, .. } | Self::Scroll { x, y, .. } => Some((*x, *y)),
      _ => None,
    }
  }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopActionRequest {
  pub action_id: Uuid,
  pub lease: LeaseToken,
  pub snapshot_id: Uuid,
  pub snapshot_revision: u64,
  pub action: DesktopAction,
}

/// An in-process host attestation, deliberately not Deserialize/Serialize.
/// Construct only after the host has authorized this exact immutable request and
/// durably stored its execution intent. Model output cannot create this proof.
pub struct AuthorizedAction {
  pub(crate) request: DesktopActionRequest,
  pub(crate) approval_id: Uuid,
}

impl AuthorizedAction {
  pub fn new(request: DesktopActionRequest, approval_id: Uuid) -> Self {
    Self {
      request,
      approval_id,
    }
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ActionOutcome {
  Completed,
  Rejected(DesktopError),
  /// The OS may have observed input. The host must persist Uncertain and never
  /// automatically replay the original action, even after reconnect/restart.
  Uncertain(DesktopError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DesktopActionReceipt {
  pub action_id: Uuid,
  pub approval_id: Uuid,
  pub snapshot_id: Uuid,
  pub outcome: ActionOutcome,
}
