use crate::{BrowserError, BrowserResult, validation::safe_profile_id};
use fs2::FileExt;
use std::{
  fs::{self, File, OpenOptions},
  path::{Path, PathBuf},
};

pub struct ProfileLease {
  directory: PathBuf,
  _lock: File,
}

impl ProfileLease {
  pub fn acquire(root: &Path, profile_id: &str) -> BrowserResult<Self> {
    safe_profile_id(profile_id)?;
    fs::create_dir_all(root).map_err(io_error)?;
    let root = root.canonicalize().map_err(io_error)?;
    private_directory(&root)?;
    let directory = root.join(profile_id);
    if fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
      return Err(BrowserError::InvalidInput(
        "profile directory cannot be a symbolic link".into(),
      ));
    }
    fs::create_dir_all(&directory).map_err(io_error)?;
    let directory = directory.canonicalize().map_err(io_error)?;
    private_directory(&directory)?;
    if !directory.starts_with(&root) {
      return Err(BrowserError::InvalidInput(
        "profile path escapes configured root".into(),
      ));
    }
    let lock = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(directory.join("lyonbrown.lock"))
      .map_err(io_error)?;
    FileExt::try_lock_exclusive(&lock).map_err(|_| {
      BrowserError::Unavailable("profile is already owned by another browser space".into())
    })?;
    Ok(Self {
      directory,
      _lock: lock,
    })
  }

  pub fn directory(&self) -> &Path {
    &self.directory
  }
}

fn io_error(error: std::io::Error) -> BrowserError {
  BrowserError::Unavailable(format!("profile storage: {error}"))
}

fn private_directory(path: &Path) -> BrowserResult<()> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
  }
  #[cfg(not(unix))]
  let _ = path;
  Ok(())
}
