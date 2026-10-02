use agent_local::DiscoveryError;
use agent_workspace::WorkspaceError;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
  #[error("failed to start local daemon: {0}")]
  Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
  #[error("daemon is unavailable")]
  Unavailable,
  #[error("daemon rejected local authentication")]
  AuthenticationRejected,
  #[error("daemon handshake is incompatible: {reason}")]
  HandshakeMismatch { reason: &'static str },
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
  #[error("invalid daemon launch request: {0}")]
  InvalidRequest(&'static str),
  #[error("secure daemon discovery failed: {0}")]
  Discovery(#[from] DiscoveryError),
  #[error("workspace identity failed: {0}")]
  Workspace(#[from] WorkspaceError),
  #[error("no reusable daemon was found and automatic startup is disabled")]
  AutoStartDisabled,
  #[error("timed out waiting for the daemon startup lock after {timeout:?}")]
  StartupLockTimeout { timeout: Duration },
  #[error("daemon did not become ready within {timeout:?}")]
  ReadinessTimeout { timeout: Duration },
  #[error("daemon exited before readiness (code {exit_code:?}): {stderr}")]
  DaemonExited {
    exit_code: Option<i32>,
    stderr: String,
  },
  #[error("daemon scope mismatch (expected {expected}, found {actual})")]
  ScopeMismatch { expected: String, actual: String },
  #[error("daemon authentication was rejected")]
  AuthenticationRejected,
  #[error("daemon handshake mismatch: {reason}")]
  HandshakeMismatch { reason: &'static str },
  #[error(transparent)]
  Spawn(#[from] SpawnError),
}
