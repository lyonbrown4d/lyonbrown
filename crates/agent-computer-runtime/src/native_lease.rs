//! Canonical per-user native desktop exclusion, independent of client data paths.
use std::{
  fs::{self, File, OpenOptions},
  path::Path,
  sync::Arc,
};

use agent_computer::DesktopIdentity;
use anyhow::{Context, ensure};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};

pub(crate) struct NativeDesktopLease {
  _file: File,
}

impl NativeDesktopLease {
  pub(crate) fn acquire(identity: &DesktopIdentity) -> anyhow::Result<Arc<Self>> {
    let directories = ProjectDirs::from("org", "Lyonbrown", "Lyonbrown")
      .context("native runtime data directory unavailable")?;
    Self::acquire_in(
      &directories.data_local_dir().join("computer-runtime"),
      identity,
    )
  }

  fn acquire_in(root: &Path, identity: &DesktopIdentity) -> anyhow::Result<Arc<Self>> {
    ensure!(
      !identity.user_session.is_empty() && identity.user_session.len() <= 1024,
      "invalid native desktop identity"
    );
    fs::create_dir_all(root).context("create native runtime exclusion directory")?;
    let directory = fs::symlink_metadata(root).context("inspect native runtime directory")?;
    ensure!(
      directory.is_dir() && !directory.file_type().is_symlink(),
      "native runtime exclusion directory must be a private directory"
    );
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(root, fs::Permissions::from_mode(0o700))
        .context("protect native runtime directory")?;
    }
    // The OS session alone identifies the physical native surface. Daemon node
    // identities and configurable client data directories must not split locks.
    let key = hex::encode(Sha256::digest(identity.user_session.as_bytes()));
    let path = root.join(format!("{key}.lock"));
    if let Ok(metadata) = fs::symlink_metadata(&path) {
      ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "native runtime exclusion path must be a regular file"
      );
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    // Never truncate or unlink lock files: replacement could create a second lock.
    let file = options
      .open(&path)
      .context("open native runtime exclusion")?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::{MetadataExt, PermissionsExt};
      let actual = file
        .metadata()
        .context("inspect native runtime exclusion")?;
      let named = fs::symlink_metadata(&path).context("inspect exclusion path")?;
      ensure!(
        named.is_file() && actual.dev() == named.dev() && actual.ino() == named.ino(),
        "native runtime exclusion path changed"
      );
      file
        .set_permissions(fs::Permissions::from_mode(0o600))
        .context("protect native runtime exclusion")?;
    }
    fs2::FileExt::try_lock_exclusive(&file)
      .context("native desktop already attached or exclusion unavailable")?;
    Ok(Arc::new(Self { _file: file }))
  }
}

/// Owns cancellation even if the async task is aborted before its first poll.
/// Native identity revocation happens while the canonical exclusion is retained.
pub(crate) struct AttachmentGuard {
  cancellation: tokio_util::sync::CancellationToken,
  runtime: Option<Arc<crate::ComputerRuntime>>,
  _exclusion: Option<Arc<NativeDesktopLease>>,
}
impl AttachmentGuard {
  pub(crate) fn new(
    cancellation: tokio_util::sync::CancellationToken,
    runtime: &Arc<crate::ComputerRuntime>,
    exclusion: Option<Arc<NativeDesktopLease>>,
  ) -> Self {
    Self {
      cancellation,
      runtime: exclusion.as_ref().map(|_| runtime.clone()),
      _exclusion: exclusion,
    }
  }
}
impl Drop for AttachmentGuard {
  fn drop(&mut self) {
    self.cancellation.cancel();
    if let Some(runtime) = &self.runtime {
      let _ = runtime.leases.revoke(&runtime.session().identity);
    }
  }
}

#[cfg(test)]
#[path = "../tests/support/native_lease.rs"]
mod tests;
