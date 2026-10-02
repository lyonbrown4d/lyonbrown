use crate::WorkspaceError;
use std::path::{Path, PathBuf};

/// Returns a machine-local identity for a workspace directory.
///
/// Equivalent path spellings and filesystem aliases resolve to the same identity.
pub fn workspace_identity(path: impl AsRef<Path>) -> Result<Vec<u8>, WorkspaceError> {
  platform_identity(path.as_ref())
}

fn io_error(path: &Path, source: std::io::Error) -> WorkspaceError {
  WorkspaceError::Io {
    path: path.to_path_buf(),
    source,
  }
}

fn ensure_directory(path: &Path) -> Result<(), WorkspaceError> {
  let metadata = path.metadata().map_err(|source| io_error(path, source))?;
  if metadata.is_dir() {
    Ok(())
  } else {
    Err(WorkspaceError::NotDirectory {
      path: PathBuf::from(path),
    })
  }
}

#[cfg(any(unix, windows))]
fn platform_identity(path: &Path) -> Result<Vec<u8>, WorkspaceError> {
  use file_id::FileId;

  ensure_directory(path)?;
  let file_id = file_id::get_file_id(path).map_err(|source| io_error(path, source))?;
  let mut identity = b"workspace-file-id-v1\0".to_vec();
  match file_id {
    FileId::Inode {
      device_id,
      inode_number,
    } => {
      identity.push(1);
      identity.extend_from_slice(&device_id.to_be_bytes());
      identity.extend_from_slice(&inode_number.to_be_bytes());
    }
    FileId::LowRes {
      volume_serial_number,
      file_index,
    } => {
      identity.push(2);
      identity.extend_from_slice(&volume_serial_number.to_be_bytes());
      identity.extend_from_slice(&file_index.to_be_bytes());
    }
    FileId::HighRes {
      volume_serial_number,
      file_id,
    } => {
      identity.push(3);
      identity.extend_from_slice(&volume_serial_number.to_be_bytes());
      identity.extend_from_slice(&file_id.to_be_bytes());
    }
  }
  Ok(identity)
}

#[cfg(not(any(unix, windows)))]
fn platform_identity(path: &Path) -> Result<Vec<u8>, WorkspaceError> {
  ensure_directory(path)?;
  let canonical = path
    .canonicalize()
    .map_err(|source| io_error(path, source))?;
  let mut identity = b"workspace-path-v1\0".to_vec();
  identity.extend_from_slice(canonical.to_string_lossy().as_bytes());
  Ok(identity)
}
