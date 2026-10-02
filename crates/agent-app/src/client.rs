use std::fmt;

use async_trait::async_trait;
use tokio::sync::watch;

use crate::{AgentCommand, RuntimeCommandError, RuntimeSnapshot};

/// Stable reason why an agent client cannot currently accept commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityKind {
  Disconnected,
  Timeout,
  ShuttingDown,
}

impl AvailabilityKind {
  pub const fn code(self) -> &'static str {
    match self {
      Self::Disconnected => "disconnected",
      Self::Timeout => "timeout",
      Self::ShuttingDown => "shutting_down",
    }
  }
}

impl fmt::Display for AvailabilityKind {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.code())
  }
}

/// Stable reason why the client and server protocol could not interoperate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolErrorKind {
  AuthenticationFailed,
  IncompatibleVersion,
  InvalidSnapshot,
  Unsupported,
}

impl ProtocolErrorKind {
  pub const fn code(self) -> &'static str {
    match self {
      Self::AuthenticationFailed => "authentication_failed",
      Self::IncompatibleVersion => "incompatible_version",
      Self::InvalidSnapshot => "invalid_snapshot",
      Self::Unsupported => "unsupported",
    }
  }
}

impl fmt::Display for ProtocolErrorKind {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.code())
  }
}

/// UI-facing failure independent of any concrete local or remote transport.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentClientError {
  #[error("agent command failed: {0}")]
  Command(#[from] RuntimeCommandError),
  #[error("agent client unavailable ({kind}): {message}")]
  Unavailable {
    kind: AvailabilityKind,
    message: String,
  },
  #[error("agent protocol failed ({kind}): {message}")]
  Protocol {
    kind: ProtocolErrorKind,
    message: String,
  },
}

impl AgentClientError {
  pub fn unavailable(kind: AvailabilityKind, message: impl Into<String>) -> Self {
    Self::Unavailable {
      kind,
      message: message.into(),
    }
  }

  pub fn protocol(kind: ProtocolErrorKind, message: impl Into<String>) -> Self {
    Self::Protocol {
      kind,
      message: message.into(),
    }
  }
}

/// Transport-independent UI port implemented by authenticated agent clients.
#[async_trait]
pub trait AgentClient: Send + Sync {
  async fn execute(&self, command: AgentCommand) -> Result<(), AgentClientError>;
  fn snapshot(&self) -> RuntimeSnapshot;
  fn subscribe(&self) -> watch::Receiver<RuntimeSnapshot>;
}
