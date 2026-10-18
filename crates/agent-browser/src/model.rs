use serde::{Deserialize, Serialize};

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_AX_BYTES: usize = 1024 * 1024;
pub const RUNTIME_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum BrowserError {
  #[error("browser capability unavailable: {0}")]
  Unavailable(String),
  #[error("invalid browser request: {0}")]
  InvalidInput(String),
  #[error("browser object not found: {0}")]
  NotFound(String),
  #[error("snapshot is stale; observe again before acting")]
  StaleSnapshot,
  #[error("human takeover is active; agent input is suspended")]
  HumanControl,
  #[error("browser operation timed out")]
  Timeout,
  #[error("browser action outcome is unknown; it was not replayed")]
  Uncertain,
  #[error("browser host protocol failed: {0}")]
  Protocol(String),
}

pub type BrowserResult<T> = Result<T, BrowserError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
  pub id: String,
  pub session_id: String,
  pub profile_id: String,
  pub human_control: bool,
  pub headless: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
  pub id: String,
  pub space_id: String,
  pub url: String,
  pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
  pub tab_id: String,
  /// Navigation/action epoch; a paint does not invalidate input coordinates.
  pub revision: u64,
  pub sequence: u64,
  pub width: u32,
  pub height: u32,
  pub stride: u32,
  /// Owned, top-left-origin BGRA8 premultiplied pixels, never a CEF pointer.
  pub bgra: Vec<u8>,
}

impl Frame {
  pub fn validate(&self) -> BrowserResult<()> {
    let stride = self
      .width
      .checked_mul(4)
      .ok_or_else(|| BrowserError::InvalidInput("frame width overflow".into()))?;
    let length = (self.stride as usize)
      .checked_mul(self.height as usize)
      .ok_or_else(|| BrowserError::InvalidInput("frame size overflow".into()))?;
    if self.width == 0
      || self.height == 0
      || self.stride != stride
      || length > MAX_FRAME_BYTES
      || self.bgra.len() != length
    {
      return Err(BrowserError::InvalidInput(
        "invalid or oversized BGRA8 frame".into(),
      ));
    }
    Ok(())
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
  pub id: String,
  pub tab_id: String,
  pub revision: u64,
  pub accessibility_json: String,
  pub frame: Option<Frame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
  pub id: String,
  pub tab_id: String,
  pub snapshot_id: String,
  pub revision: u64,
  /// Supported semantic actions: click, fill, press, navigate.
  pub kind: String,
  pub element_ref: String,
  pub text: String,
  pub timeout_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ViewInput {
  MouseMove {
    x: i32,
    y: i32,
  },
  MouseButton {
    x: i32,
    y: i32,
    button: u8,
    pressed: bool,
  },
  Wheel {
    x: i32,
    y: i32,
    delta_x: i32,
    delta_y: i32,
  },
  Key {
    virtual_key: i32,
    modifiers: u32,
    pressed: bool,
  },
  Text(String),
  Resize {
    width: u32,
    height: u32,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
  Hello,
  OpenSpace {
    session_id: String,
    profile_id: String,
    headless: bool,
  },
  ListSpaces,
  OpenTab {
    space_id: String,
    url: String,
  },
  Observe {
    tab_id: String,
    screenshot: bool,
  },
  Act(Action),
  Takeover {
    space_id: String,
    human_control: bool,
  },
  CloseSpace {
    space_id: String,
  },
  GetFrame {
    tab_id: String,
    after_sequence: u64,
  },
  Input {
    tab_id: String,
    revision: u64,
    event: ViewInput,
  },
  Shutdown,
}

impl Command {
  pub fn read_only(&self) -> bool {
    matches!(
      self,
      Self::Hello | Self::ListSpaces | Self::Observe { .. } | Self::GetFrame { .. }
    )
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
  Ready {
    engine: String,
    protocol_version: u32,
  },
  Space(Space),
  Spaces(Vec<Space>),
  Tab(Tab),
  Snapshot(Snapshot),
  Frame(Option<Frame>),
  Done,
}
