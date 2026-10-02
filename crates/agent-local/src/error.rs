use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
  #[error("discovery descriptor exceeds {limit} bytes: {actual} bytes")]
  DescriptorTooLarge { actual: usize, limit: usize },
  #[error("invalid discovery descriptor: {0}")]
  InvalidDescriptor(String),
  #[error("daemon address must be numeric loopback: {0}")]
  NonLoopbackAddress(std::net::SocketAddr),
  #[error("daemon address must use a non-zero port")]
  ZeroPort,
  #[error("daemon pid must be non-zero")]
  ZeroPid,
  #[error("invalid local authentication token")]
  InvalidToken,
  #[error("cryptographically secure token generation failed")]
  TokenGeneration,
  #[error("symbolic link is not allowed at {0}")]
  SymlinkRejected(PathBuf),
  #[error("run directory is not owned by the current user: {path}")]
  WrongOwner { path: PathBuf },
  #[error("insecure permissions at {path}: expected {expected:o}, found {actual:o}")]
  InsecurePermissions {
    path: PathBuf,
    expected: u32,
    actual: u32,
  },
  #[error("insecure Windows access control at {path}: expected a protected current-user-only DACL")]
  InsecureAccessControl { path: PathBuf },
  #[error("path is not the required file type: {0}")]
  WrongFileType(PathBuf),
  #[error(
    "secure local discovery is unsupported on {platform} without a verified owner-only boundary"
  )]
  UnsupportedSecurityBoundary { platform: &'static str },
  #[error("local discovery I/O failed while {operation}: {source}")]
  Io {
    operation: &'static str,
    #[source]
    source: std::io::Error,
  },
}

impl DiscoveryError {
  pub(crate) fn io(operation: &'static str, source: std::io::Error) -> Self {
    Self::Io { operation, source }
  }
}
