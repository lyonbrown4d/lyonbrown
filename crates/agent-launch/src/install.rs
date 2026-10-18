use std::{fs, path::Path};

#[cfg(not(windows))]
use anyhow::Context;
use anyhow::{Result, ensure};

use crate::model::Registration;

/// Retain versioned daemon binaries so removing either client never removes the service.
#[cfg(not(windows))]
pub fn retain_binary(candidate: &Registration) -> Result<Registration> {
  let directory = candidate
    .data_dir
    .join("daemon")
    .join(candidate.version.to_string());
  fs::create_dir_all(&directory)?;
  let name = if cfg!(windows) {
    "agentd.exe"
  } else {
    "agentd"
  };
  let destination = directory.join(name);
  if destination.exists() {
    ensure!(
      fs::read(&destination)? == fs::read(&candidate.executable)?,
      "a different daemon binary is already installed with version {}; refusing same-version replacement",
      candidate.version
    );
  } else {
    let pending = directory.join(format!("{name}.pending"));
    fs::copy(&candidate.executable, &pending).context("stage bundled agentd")?;
    fs::File::open(&pending)?.sync_all()?;
    fs::rename(pending, &destination)?;
    sync_directory(&directory)?;
  }
  let mut registration = candidate.clone();
  registration.executable = destination.canonicalize()?;
  Ok(registration)
}

/// Windows Installer owns protected machine binaries. Clients never promote
/// user-writable files into an elevated SCM service installation.
#[cfg(windows)]
pub fn retain_binary(candidate: &Registration) -> Result<Registration> {
  let destination = crate::native::windows::daemon_directory()?
    .join(candidate.version.to_string())
    .join("agentd.exe");
  ensure!(
    destination.is_file(),
    "protected agentd version is missing; install the complete native Lyonbrown MSI before registering the Windows service"
  );
  let mut registration = candidate.clone();
  registration.executable = destination.canonicalize()?;
  crate::native::windows::validate_executable(&registration)?;
  ensure!(
    same_binary(&candidate.executable, &registration.executable)?,
    "installed daemon differs from this bundle; repair the native MSI instead of replacing a protected service binary"
  );
  Ok(registration)
}

#[cfg(windows)]
fn same_binary(first: &Path, second: &Path) -> Result<bool> {
  use std::io::Read;
  let mut first = fs::File::open(first)?;
  let mut second = fs::File::open(second)?;
  if first.metadata()?.len() != second.metadata()?.len() {
    return Ok(false);
  }
  let mut left = [0_u8; 65_536];
  let mut right = [0_u8; 65_536];
  loop {
    let count = first.read(&mut left)?;
    if count == 0 {
      return Ok(true);
    }
    second.read_exact(&mut right[..count])?;
    if left[..count] != right[..count] {
      return Ok(false);
    }
  }
}

#[cfg(not(windows))]
fn sync_directory(path: &Path) -> Result<()> {
  #[cfg(unix)]
  fs::File::open(path)?.sync_all()?;
  #[cfg(not(unix))]
  let _ = path;
  Ok(())
}
