use crate::{DiscoveryError, SecureRunDirectory};

#[derive(Debug)]
pub(crate) struct FileLock {
  file: std::fs::File,
}

impl FileLock {
  #[cfg(unix)]
  pub(crate) fn try_acquire(
    run_dir: &SecureRunDirectory,
    file_name: &str,
    activity: &'static str,
  ) -> Result<Option<Self>, DiscoveryError> {
    use crate::filesystem::validate_private_file;
    use std::fs::TryLockError;

    let path = run_dir.path().join(file_name);
    let file =
      crate::unix_filesystem::open_private_lock(run_dir.handle(), file_name, &path, activity)?;
    validate_private_file(
      &path,
      &file
        .metadata()
        .map_err(|error| DiscoveryError::io("reading lock metadata", error))?,
    )?;
    match file.try_lock() {
      Ok(()) => Ok(Some(Self { file })),
      Err(TryLockError::WouldBlock) => Ok(None),
      Err(TryLockError::Error(error)) => Err(DiscoveryError::io(activity, error)),
    }
  }

  #[cfg(not(unix))]
  #[cfg(windows)]
  pub(crate) fn try_acquire(
    run_dir: &SecureRunDirectory,
    file_name: &str,
    activity: &'static str,
  ) -> Result<Option<Self>, DiscoveryError> {
    use std::fs::TryLockError;

    let path = run_dir.path().join(file_name);
    let file =
      crate::windows_security::open_private_lock(run_dir.windows_handle(), file_name, &path)?;
    match file.try_lock() {
      Ok(()) => Ok(Some(Self { file })),
      Err(TryLockError::WouldBlock) => Ok(None),
      Err(TryLockError::Error(error)) => Err(DiscoveryError::io(activity, error)),
    }
  }

  #[cfg(not(any(unix, windows)))]
  pub(crate) fn try_acquire(
    _run_dir: &SecureRunDirectory,
    _file_name: &str,
    _activity: &'static str,
  ) -> Result<Option<Self>, DiscoveryError> {
    Err(DiscoveryError::UnsupportedSecurityBoundary {
      platform: std::env::consts::OS,
    })
  }
}

impl Drop for FileLock {
  fn drop(&mut self) {
    let _ = self.file.unlock();
  }
}
