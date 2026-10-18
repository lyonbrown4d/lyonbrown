use std::path::Path;

use anyhow::Result;

#[cfg(target_os = "macos")]
#[path = "pkg.rs"]
mod pkg;

/// Delegate archive formats to host packaging tools; never synthesize an installer format.
pub fn package(
  root: &Path,
  binaries: &Path,
  output: &Path,
  version: &str,
  cli_only: bool,
  cef_runtime: Option<&Path>,
) -> Result<()> {
  if cfg!(target_os = "macos") {
    macos(binaries, output, version, cli_only)
  } else if cfg!(target_os = "linux") {
    crate::linux::package(root, binaries, output, version, cli_only, cef_runtime)
  } else if cfg!(target_os = "windows") {
    crate::windows::package(root, binaries, output, version, cli_only, None)
  } else {
    anyhow::bail!("native installers are unavailable on this platform")
  }
}

fn macos(binaries: &Path, output: &Path, version: &str, cli_only: bool) -> Result<()> {
  #[cfg(target_os = "macos")]
  {
    pkg::package(binaries, output, version, cli_only)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (binaries, output, version, cli_only);
    anyhow::bail!("Apple PKG creation requires a macOS host")
  }
}
