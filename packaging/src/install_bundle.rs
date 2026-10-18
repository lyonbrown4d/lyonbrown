//! Versioned macOS CLI bundle installation, separate from service registration.
use std::{
  fs,
  os::unix::fs::{PermissionsExt, symlink},
  path::{Path, PathBuf},
  process::Command,
  sync::atomic::{AtomicU64, Ordering},
};

use agent_launch::bundle_identity::bundle_identity;
use anyhow::{Context, Result, ensure};
use semver::Version;

pub fn install_native_cli(source: &Path, destination: &Path, version: &Version) -> Result<()> {
  install_with_copy(source, destination, version, |bundle, pending| {
    let status = Command::new("ditto").arg(bundle).arg(pending).status()?;
    ensure!(status.success(), "native CLI app installation failed");
    Ok(())
  })
}

pub(crate) fn install_with_copy(
  source: &Path,
  destination: &Path,
  version: &Version,
  copy: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<()> {
  let destination = destination
    .canonicalize()
    .context("resolve CLI bin directory")?;
  let bundle = source
    .parent()
    .context("CLI archive has no root")?
    .join("Lyonbrown CLI.app");
  let identity = bundle_identity(&bundle)?;
  ensure!(
    bundle.join("Contents/MacOS/agent-cli").is_file(),
    "native CLI app is missing its executable"
  );
  ensure!(
    fs::metadata(bundle.join("Contents/MacOS/agent-cli"))?
      .permissions()
      .mode()
      & 0o111
      != 0,
    "native CLI app has no execute permission"
  );
  let versions = destination
    .parent()
    .context("bin directory has no parent")?
    .join("share/lyonbrown/cli");
  fs::create_dir_all(&versions)?;
  // Preserve previous bundles while their clients and helpers are still running.
  let parent = versions.join(format!("{version}-{identity}"));
  let installed = parent.join("Lyonbrown CLI.app");
  match fs::symlink_metadata(&installed) {
    Ok(_) => ensure!(
      bundle_identity(&installed)? == identity,
      "retained native CLI app is corrupt; refusing replacement"
    ),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      fs::create_dir_all(&parent)?;
      let pending = Pending::directory(&parent)?;
      copy(&bundle, &pending.path)?;
      ensure!(
        bundle_identity(&pending.path)? == identity,
        "copied native CLI app differs from selected bundle"
      );
      fs::rename(&pending.path, &installed)?;
    }
    Err(error) => return Err(error.into()),
  }
  let pending = Pending::link(&destination, &installed.join("Contents/MacOS/agent-cli"))?;
  fs::rename(&pending.path, destination.join("agent"))?;
  Ok(())
}

static NEXT_PENDING: AtomicU64 = AtomicU64::new(0);

struct Pending {
  path: PathBuf,
  directory: bool,
}
impl Pending {
  fn path(parent: &Path) -> PathBuf {
    parent.join(format!(
      ".agent-install-{}-{}",
      std::process::id(),
      NEXT_PENDING.fetch_add(1, Ordering::Relaxed)
    ))
  }

  fn directory(parent: &Path) -> Result<Self> {
    let path = Self::path(parent);
    fs::create_dir(&path)?;
    Ok(Self {
      path,
      directory: true,
    })
  }

  fn link(parent: &Path, target: &Path) -> Result<Self> {
    let path = Self::path(parent);
    symlink(target, &path)?;
    Ok(Self {
      path,
      directory: false,
    })
  }
}
impl Drop for Pending {
  fn drop(&mut self) {
    // Only remove paths exclusively created by this attempt, never retained apps.
    if self.directory {
      let _ = fs::remove_dir_all(&self.path);
    } else {
      let _ = fs::remove_file(&self.path);
    }
  }
}
