use std::{
  fs::{self, File, OpenOptions},
  path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use fs2::FileExt;

use crate::model::ManagedState;

/// Serialize CLI and Desktop registration across processes for the same user.
pub struct ServiceLock {
  _file: File,
  state_path: PathBuf,
}

impl ServiceLock {
  /// The native service identity is per user, even when clients choose different
  /// daemon data directories. Its lock and registration must share that scope.
  pub fn for_current_user() -> Result<Self> {
    Self::acquire(&crate::options::default_data_dir()?)
  }

  /// Acquire a registry in an explicit directory (for isolated service managers).
  pub fn acquire(data_dir: &Path) -> Result<Self> {
    fs::create_dir_all(data_dir)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(data_dir, fs::Permissions::from_mode(0o700))?;
    }
    let file = OpenOptions::new()
      .create(true)
      .truncate(false)
      .read(true)
      .write(true)
      .open(data_dir.join("service.lock"))?;
    file
      .try_lock_exclusive()
      .context("another client is registering agentd; retry after it completes")?;
    Ok(Self {
      _file: file,
      state_path: data_dir.join("service.json"),
    })
  }

  pub fn read(&self) -> Result<Option<ManagedState>> {
    match fs::read(&self.state_path) {
      Ok(bytes) => Ok(Some(
        serde_json::from_slice(&bytes).context("invalid native service state")?,
      )),
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
      Err(error) => Err(error.into()),
    }
  }

  pub fn write(&self, state: &ManagedState) -> Result<()> {
    let pending = self.state_path.with_extension("json.pending");
    let mut file = OpenOptions::new()
      .create(true)
      .truncate(true)
      .write(true)
      .open(&pending)?;
    serde_json::to_writer_pretty(&mut file, state)?;
    file.sync_all()?;
    fs::rename(pending, &self.state_path)?;
    #[cfg(unix)]
    if let Some(parent) = self.state_path.parent() {
      File::open(parent)?.sync_all()?;
    }
    Ok(())
  }

  pub fn clear(&self) -> Result<()> {
    fs::remove_file(&self.state_path)?;
    Ok(())
  }
}
