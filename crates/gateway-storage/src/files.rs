use crate::{Result, StorageError};
use fs2::FileExt;
use std::{
  fs::{DirBuilder, File, OpenOptions},
  path::Path,
};

pub(crate) fn prepare(path: &Path) -> Result<File> {
  let parent = path
    .parent()
    .filter(|p| !p.as_os_str().is_empty())
    .unwrap_or(Path::new("."));
  let mut directories = DirBuilder::new();
  directories.recursive(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::DirBuilderExt;
    directories.mode(0o700);
  }
  directories.create(parent)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::metadata(parent)?.permissions().mode() & 0o022 != 0 {
      return Err(StorageError::InvalidInput(
        "database parent must not be writable by other users",
      ));
    }
  }
  let lock_path = path.with_extension("ownership.lock");
  for candidate in [path, lock_path.as_path()] {
    if std::fs::symlink_metadata(candidate).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
      return Err(StorageError::InvalidInput(
        "database and lock must not be symlinks",
      ));
    }
  }
  let file = open_private(path)?;
  drop(file);
  let ownership = open_private(&lock_path)?;
  ownership
    .try_lock_exclusive()
    .map_err(|_| StorageError::Conflict)?;
  Ok(ownership)
}
fn open_private(path: &Path) -> Result<File> {
  let mut options = OpenOptions::new();
  options.create(true).truncate(false).read(true).write(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  let file = options.open(path)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
  }
  Ok(file)
}
