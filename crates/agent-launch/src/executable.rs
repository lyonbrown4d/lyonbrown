//! Platform boundary for the file actually loaded by the current process.
use std::path::PathBuf;

use anyhow::Result;

#[cfg(target_os = "macos")]
mod macos;

/// Locate the current process image without selecting a different client version.
pub fn current_executable() -> Result<PathBuf> {
  #[cfg(target_os = "macos")]
  {
    macos::current_executable()
  }
  #[cfg(not(target_os = "macos"))]
  {
    use anyhow::Context;
    std::env::current_exe().context("locate client executable")
  }
}
