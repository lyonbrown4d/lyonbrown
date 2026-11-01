use std::{
  fs::{self, File, OpenOptions},
  io::{Read, Write},
  path::Path,
};

use tempfile::NamedTempFile;

use crate::{Result, SecretError};

pub(crate) fn root(path: &Path) -> Result<()> {
  fs::create_dir_all(path)?;
  if !fs::symlink_metadata(path)?.file_type().is_dir() {
    return Err(SecretError::UnsupportedEntry);
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
  }
  Ok(())
}

pub(crate) fn lock(root: &Path) -> Result<File> {
  let mut options = OpenOptions::new();
  options.create(true).truncate(false).read(true).write(true);
  private_options(&mut options);
  let file = options.open(root.join(".lock"))?;
  check_private(&file)?;
  fs2::FileExt::lock_exclusive(&file)?;
  Ok(file)
}

pub(crate) fn read(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
  match fs::symlink_metadata(path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
    Ok(metadata) if !metadata.file_type().is_file() => return Err(SecretError::UnsupportedEntry),
    Ok(_) => {}
  }
  let mut options = OpenOptions::new();
  options.read(true);
  private_options(&mut options);
  let file = options.open(path)?;
  check_private(&file)?;
  let mut bytes = Vec::new();
  file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
  if bytes.len() > limit {
    return Err(SecretError::InvalidRecord);
  }
  Ok(Some(bytes))
}

pub(crate) fn atomic_write(root: &Path, target: &Path, bytes: &[u8], create: bool) -> Result<()> {
  let mut file = NamedTempFile::with_prefix_in(".pending-", root)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    file
      .as_file()
      .set_permissions(fs::Permissions::from_mode(0o600))?;
  }
  file.write_all(bytes)?;
  file.as_file().sync_all()?;
  if create {
    file
      .persist_noclobber(target)
      .map_err(|error| error.error)?;
  } else {
    file.persist(target).map_err(|error| error.error)?;
  }
  sync(root)
}

pub(crate) fn sync(root: &Path) -> Result<()> {
  #[cfg(unix)]
  File::open(root)?.sync_all()?;
  #[cfg(not(unix))]
  let _ = root;
  Ok(())
}

fn private_options(options: &mut OpenOptions) {
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
  }
  #[cfg(not(unix))]
  let _ = options;
}

fn check_private(file: &File) -> Result<()> {
  let metadata = file.metadata()?;
  if !metadata.file_type().is_file() {
    return Err(SecretError::UnsupportedEntry);
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
      return Err(SecretError::Permissions);
    }
  }
  Ok(())
}
