use crate::DiscoveryError;
use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use std::ffi::OsStr;
use std::fs::File;
use std::path::{Component, Path, PathBuf};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
  .union(OFlags::DIRECTORY)
  .union(OFlags::NOFOLLOW)
  .union(OFlags::CLOEXEC);
const PRIVATE_FILE_MODE: Mode = Mode::RUSR.union(Mode::WUSR);

pub(crate) fn open_run_directory(path: &Path) -> Result<(PathBuf, File), DiscoveryError> {
  let absolute = std::path::absolute(path)
    .map_err(|error| DiscoveryError::io("resolving run directory path", error))?;
  let root = rustix::fs::open("/", DIRECTORY_FLAGS, Mode::empty())
    .map(File::from)
    .map_err(|error| DiscoveryError::io("opening filesystem root", error.into()))?;
  let mut directory = root;
  let mut traversed = PathBuf::from("/");

  for component in absolute.components() {
    let name = match component {
      Component::RootDir | Component::CurDir => continue,
      Component::Normal(name) => name,
      Component::ParentDir | Component::Prefix(_) => {
        return Err(DiscoveryError::io(
          "resolving run directory path",
          std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "run directory path must not contain parent components",
          ),
        ));
      }
    };
    traversed.push(name);
    directory = open_or_create_directory(&directory, name, &traversed)?;
  }
  Ok((absolute, directory))
}

fn open_or_create_directory(
  parent: &File,
  name: &OsStr,
  path: &Path,
) -> Result<File, DiscoveryError> {
  reject_symlink(parent, name, path)?;
  match open_directory(parent, name) {
    Ok(directory) => Ok(directory),
    Err(error) if error == rustix::io::Errno::NOENT => {
      match rustix::fs::mkdirat(parent, name, Mode::RWXU) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(error) => {
          return Err(DiscoveryError::io("creating run directory", error.into()));
        }
      }
      reject_symlink(parent, name, path)?;
      open_directory(parent, name).map_err(|error| open_error("opening run directory", path, error))
    }
    Err(error) => Err(open_error("opening run directory", path, error)),
  }
}

fn open_directory(parent: &File, name: &OsStr) -> rustix::io::Result<File> {
  rustix::fs::openat(parent, name, DIRECTORY_FLAGS, Mode::empty()).map(File::from)
}

pub(crate) fn create_private_file(
  directory: &File,
  name: &str,
  path: &Path,
  activity: &'static str,
) -> Result<File, DiscoveryError> {
  open_file(
    directory,
    name,
    OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
    path,
    activity,
  )
}

pub(crate) fn open_private_read(
  directory: &File,
  name: &str,
  path: &Path,
) -> Result<File, DiscoveryError> {
  open_file(
    directory,
    name,
    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
    path,
    "opening discovery descriptor",
  )
}

pub(crate) fn open_private_lock(
  directory: &File,
  name: &str,
  path: &Path,
  activity: &'static str,
) -> Result<File, DiscoveryError> {
  open_file(
    directory,
    name,
    OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
    path,
    activity,
  )
}

fn open_file(
  directory: &File,
  name: &str,
  flags: OFlags,
  path: &Path,
  activity: &'static str,
) -> Result<File, DiscoveryError> {
  reject_symlink(directory, OsStr::new(name), path)?;
  rustix::fs::openat(directory, name, flags, PRIVATE_FILE_MODE)
    .map(File::from)
    .map_err(|error| open_error(activity, path, error))
}

pub(crate) fn rename(directory: &File, from: &str, to: &str) -> Result<(), DiscoveryError> {
  rustix::fs::renameat(directory, from, directory, to)
    .map_err(|error| DiscoveryError::io("publishing discovery descriptor", error.into()))
}

pub(crate) fn remove(directory: &File, name: &str) {
  let _ = rustix::fs::unlinkat(directory, name, AtFlags::empty());
}

fn reject_symlink(parent: &File, name: &OsStr, path: &Path) -> Result<(), DiscoveryError> {
  match rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
    Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Symlink => {
      Err(DiscoveryError::SymlinkRejected(path.to_path_buf()))
    }
    Ok(_) | Err(rustix::io::Errno::NOENT) => Ok(()),
    Err(error) => Err(DiscoveryError::io("inspecting secure path", error.into())),
  }
}

fn open_error(activity: &'static str, path: &Path, error: rustix::io::Errno) -> DiscoveryError {
  if error == rustix::io::Errno::LOOP {
    DiscoveryError::SymlinkRejected(path.to_path_buf())
  } else {
    DiscoveryError::io(activity, error.into())
  }
}
