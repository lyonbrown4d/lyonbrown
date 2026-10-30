//! Private, client-owned snapshots for restoring presentation state.
//!
//! This store must only contain client DTOs such as selected tabs or filters. It
//! is not a source of credentials, daemon task state, or execution authority.
use std::{
  fs::{self, File, OpenOptions},
  io::{Read, Write},
  path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

mod identity;
pub use identity::identity;

const SCHEMA_VERSION: u32 = 1;
const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;

/// A locked store for one client kind and authenticated daemon identity.
///
/// The exclusive lock is held for this value's lifetime. Concurrent owners for
/// the same client and daemon fail at construction rather than overwrite state.
pub struct LocalStateStore {
  file_path: PathBuf,
  identity: String,
  _lock: File,
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
  schema_version: u32,
  identity: String,
  payload: T,
}

impl LocalStateStore {
  /// Opens the private snapshot store below the supplied user data directory.
  pub fn new(root: PathBuf, client_kind: &str, endpoint: &str, node_id: &str) -> Result<Self> {
    let identity = identity(client_kind, endpoint, node_id)?;

    fs::create_dir_all(&root).context("create client data directory")?;
    let root = fs::canonicalize(&root).context("resolve client data directory")?;
    let state_dir = root.join("client-state");
    match fs::symlink_metadata(&state_dir) {
      Ok(metadata) => ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "client state directory is not a real directory"
      ),
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
        fs::create_dir(&state_dir).context("create client state directory")?
      }
      Err(error) => return Err(error).context("inspect client state directory"),
    }
    set_private_dir(&state_dir)?;

    let file_path = state_dir.join(format!("{client_kind}-{identity}.json"));
    let lock_path = state_dir.join(format!("{client_kind}-{identity}.lock"));
    let lock = open_lock(&lock_path)?;
    lock
      .try_lock_exclusive()
      .context("client state is already owned by another process")?;

    Ok(Self {
      file_path,
      identity,
      _lock: lock,
    })
  }

  /// Loads a previously saved client-owned DTO, if one exists.
  pub fn load<T: DeserializeOwned>(&self) -> Result<Option<T>> {
    match fs::symlink_metadata(&self.file_path) {
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
      Err(error) => return Err(error).context("inspect client state file"),
      Ok(metadata) => ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "client state is not a regular file"
      ),
    }
    let file = open_readonly_nofollow(&self.file_path)?;
    let mut bytes = Vec::with_capacity(4096);
    set_private_file(&file)?;
    file
      .take(MAX_STATE_BYTES + 1)
      .read_to_end(&mut bytes)
      .context("read client state")?;
    ensure!(
      bytes.len() as u64 <= MAX_STATE_BYTES,
      "client state exceeds size limit"
    );
    let envelope: Envelope<T> = serde_json::from_slice(&bytes).context("decode client state")?;
    ensure!(
      envelope.schema_version == SCHEMA_VERSION,
      "unsupported client state schema version {}",
      envelope.schema_version
    );
    ensure!(
      envelope.identity == self.identity,
      "client state identity mismatch"
    );
    Ok(Some(envelope.payload))
  }

  /// Atomically saves a client-owned DTO with private file permissions.
  pub fn save<T: Serialize>(&self, state: &T) -> Result<()> {
    self.validate_existing()?;
    let bytes = serde_json::to_vec(&Envelope {
      schema_version: SCHEMA_VERSION,
      identity: self.identity.clone(),
      payload: state,
    })
    .context("serialize client state")?;
    ensure!(
      bytes.len() as u64 <= MAX_STATE_BYTES,
      "client state exceeds size limit"
    );
    ensure_state_target(&self.file_path)?;

    let parent = self
      .file_path
      .parent()
      .context("client state path has no parent")?;
    let mut temporary =
      NamedTempFile::new_in(parent).context("create temporary client state file")?;
    set_private_file(temporary.as_file())?;
    temporary
      .write_all(&bytes)
      .context("write temporary client state")?;
    temporary
      .as_file()
      .sync_all()
      .context("sync temporary client state")?;
    temporary
      .persist(&self.file_path)
      .context("atomically replace client state")?;
    sync_parent_directory(parent)?;
    Ok(())
  }

  fn validate_existing(&self) -> Result<()> {
    match fs::symlink_metadata(&self.file_path) {
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
      Err(error) => return Err(error).context("inspect existing client state"),
      Ok(metadata) => ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "client state is not a regular file"
      ),
    }
    let file = open_readonly_nofollow(&self.file_path)?;
    let mut bytes = Vec::with_capacity(4096);
    set_private_file(&file)?;
    file
      .take(MAX_STATE_BYTES + 1)
      .read_to_end(&mut bytes)
      .context("read existing client state")?;
    ensure!(
      bytes.len() as u64 <= MAX_STATE_BYTES,
      "existing client state exceeds size limit"
    );
    let envelope: Envelope<serde_json::Value> =
      serde_json::from_slice(&bytes).context("decode existing client state")?;
    ensure!(
      envelope.schema_version == SCHEMA_VERSION,
      "unsupported client state schema version {}",
      envelope.schema_version
    );
    ensure!(
      envelope.identity == self.identity,
      "client state identity mismatch"
    );
    Ok(())
  }
}

fn ensure_state_target(path: &Path) -> Result<()> {
  match fs::symlink_metadata(path) {
    Ok(metadata) => {
      ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "client state is not a regular file"
      );
      Ok(())
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error).context("inspect client state target"),
  }
}

#[cfg(unix)]
fn set_private_dir(path: &Path) -> Result<()> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    .context("secure client state directory")
}
#[cfg(not(unix))]
fn set_private_dir(_path: &Path) -> Result<()> {
  Ok(())
}

#[cfg(unix)]
fn set_private_file(file: &File) -> Result<()> {
  use std::os::unix::fs::PermissionsExt;
  file
    .set_permissions(fs::Permissions::from_mode(0o600))
    .context("secure client state file")
}
#[cfg(not(unix))]
fn set_private_file(_file: &File) -> Result<()> {
  Ok(())
}

fn open_lock(path: &Path) -> Result<File> {
  ensure_state_target(path)?;
  let mut options = OpenOptions::new();
  options.create(true).read(true).write(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
  }
  let file = options.open(path).context("open client state lock")?;
  set_private_file(&file)?;
  Ok(file)
}

fn open_readonly_nofollow(path: &Path) -> Result<File> {
  let mut options = OpenOptions::new();
  options.read(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(libc::O_NOFOLLOW);
  }
  options.open(path).context("open client state file")
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> Result<()> {
  File::open(parent)
    .and_then(|directory| directory.sync_all())
    .context("sync client state directory")
}

#[cfg(windows)]
fn sync_parent_directory(_parent: &Path) -> Result<()> {
  // The snapshot file is flushed before tempfile::persist replaces the target
  // from a temporary file in the same directory. Windows does not permit the
  // directory-handle fsync used on Unix, so directory durability is delegated
  // to the platform's same-volume replace operation.
  Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_parent_directory(_parent: &Path) -> Result<()> {
  Ok(())
}
