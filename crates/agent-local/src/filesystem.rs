#[cfg(unix)]
use crate::MAX_DISCOVERY_BYTES;
#[cfg(any(unix, windows))]
use crate::SecretToken;
use crate::{DiscoveryDescriptor, DiscoveryError};
use std::path::{Path, PathBuf};

const DISCOVERY_FILE: &str = "agentd.json";

#[derive(Debug)]
pub struct SecureRunDirectory {
  path: PathBuf,
  #[cfg(unix)]
  handle: std::fs::File,
  #[cfg(windows)]
  handle: cap_std::fs::Dir,
}

impl SecureRunDirectory {
  #[cfg(windows)]
  pub fn open(path: impl AsRef<Path>) -> Result<Self, DiscoveryError> {
    let (path, handle) = crate::windows_security::open_run_directory(path.as_ref())?;
    Ok(Self {
      path,
      handle: cap_std::fs::Dir::from_std_file(handle),
    })
  }

  #[cfg(unix)]
  pub fn open(path: impl AsRef<Path>) -> Result<Self, DiscoveryError> {
    use std::os::unix::fs::MetadataExt as _;

    let (path, handle) = crate::unix_filesystem::open_run_directory(path.as_ref())?;
    let metadata = handle
      .metadata()
      .map_err(|error| DiscoveryError::io("reading run directory metadata", error))?;
    if !metadata.is_dir() {
      return Err(DiscoveryError::WrongFileType(path));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
      return Err(DiscoveryError::WrongOwner { path });
    }
    let actual = metadata.mode() & 0o777;
    if actual != 0o700 {
      return Err(DiscoveryError::InsecurePermissions {
        path,
        expected: 0o700,
        actual,
      });
    }
    Ok(Self { path, handle })
  }

  #[cfg(not(any(unix, windows)))]
  pub fn open(_path: impl AsRef<Path>) -> Result<Self, DiscoveryError> {
    Err(DiscoveryError::UnsupportedSecurityBoundary {
      platform: std::env::consts::OS,
    })
  }

  pub fn path(&self) -> &Path {
    &self.path
  }

  #[cfg(unix)]
  pub(crate) fn handle(&self) -> &std::fs::File {
    &self.handle
  }

  #[cfg(windows)]
  pub(crate) fn windows_handle(&self) -> &cap_std::fs::Dir {
    &self.handle
  }
}

#[derive(Debug)]
pub struct DiscoveryStore {
  run_dir: SecureRunDirectory,
}

impl DiscoveryStore {
  pub fn new(run_dir: SecureRunDirectory) -> Self {
    Self { run_dir }
  }

  pub fn path(&self) -> PathBuf {
    self.run_dir.path().join(DISCOVERY_FILE)
  }

  #[cfg(unix)]
  pub fn publish(&self, descriptor: &DiscoveryDescriptor) -> Result<(), DiscoveryError> {
    use std::io::Write as _;

    let encoded = descriptor.to_json()?;
    let nonce = SecretToken::generate()?.expose_for_transport();
    let temporary_name = format!(".agentd-{}.tmp", &nonce[..16]);
    let temporary_path = self.run_dir.path().join(&temporary_name);
    let mut file = crate::unix_filesystem::create_private_file(
      self.run_dir.handle(),
      &temporary_name,
      &temporary_path,
      "creating discovery temporary file",
    )?;
    let result = (|| {
      validate_private_file(
        &temporary_path,
        &file
          .metadata()
          .map_err(|error| DiscoveryError::io("reading temporary file metadata", error))?,
      )?;
      file
        .write_all(&encoded)
        .map_err(|error| DiscoveryError::io("writing discovery descriptor", error))?;
      file
        .sync_all()
        .map_err(|error| DiscoveryError::io("syncing discovery descriptor", error))?;
      crate::unix_filesystem::rename(self.run_dir.handle(), &temporary_name, DISCOVERY_FILE)?;
      self
        .run_dir
        .handle()
        .sync_all()
        .map_err(|error| DiscoveryError::io("syncing run directory", error))
    })();
    if result.is_err() {
      crate::unix_filesystem::remove(self.run_dir.handle(), &temporary_name);
    }
    result
  }

  #[cfg(windows)]
  pub fn publish(&self, descriptor: &DiscoveryDescriptor) -> Result<(), DiscoveryError> {
    use std::io::Write as _;

    let encoded = descriptor.to_json()?;
    let nonce = SecretToken::generate()?.expose_for_transport();
    let temporary = format!(".agentd-{}.tmp", &nonce[..16]);
    let mut file = crate::windows_security::create_private_file(
      self.run_dir.windows_handle(),
      &temporary,
      &self.run_dir.path().join(&temporary),
    )?;
    let result = (|| {
      file
        .write_all(&encoded)
        .map_err(|error| DiscoveryError::io("writing discovery descriptor", error))?;
      file
        .sync_all()
        .map_err(|error| DiscoveryError::io("syncing discovery descriptor", error))?;
      self
        .run_dir
        .windows_handle()
        .rename(&temporary, self.run_dir.windows_handle(), DISCOVERY_FILE)
        .map_err(|error| DiscoveryError::io("publishing discovery descriptor", error))?;
      crate::windows_security::validate_private_file(&self.path(), &file)
    })();
    if result.is_err() {
      let _ = self.run_dir.windows_handle().remove_file(&temporary);
    }
    result
  }

  #[cfg(not(any(unix, windows)))]
  pub fn publish(&self, _descriptor: &DiscoveryDescriptor) -> Result<(), DiscoveryError> {
    Err(DiscoveryError::UnsupportedSecurityBoundary {
      platform: std::env::consts::OS,
    })
  }

  #[cfg(unix)]
  pub fn read(&self) -> Result<DiscoveryDescriptor, DiscoveryError> {
    use std::io::Read as _;

    let path = self.path();
    let file =
      crate::unix_filesystem::open_private_read(self.run_dir.handle(), DISCOVERY_FILE, &path)?;
    validate_private_file(
      &path,
      &file
        .metadata()
        .map_err(|error| DiscoveryError::io("reading discovery descriptor metadata", error))?,
    )?;
    let mut encoded = Vec::with_capacity(512);
    file
      .take((MAX_DISCOVERY_BYTES + 1) as u64)
      .read_to_end(&mut encoded)
      .map_err(|error| DiscoveryError::io("reading discovery descriptor", error))?;
    DiscoveryDescriptor::from_json(&encoded)
  }

  #[cfg(windows)]
  pub fn read(&self) -> Result<DiscoveryDescriptor, DiscoveryError> {
    use std::io::Read as _;

    let path = self.path();
    let file = crate::windows_security::open_private_read(
      self.run_dir.windows_handle(),
      DISCOVERY_FILE,
      &path,
    )?;
    let mut encoded = Vec::with_capacity(512);
    file
      .take((crate::MAX_DISCOVERY_BYTES + 1) as u64)
      .read_to_end(&mut encoded)
      .map_err(|error| DiscoveryError::io("reading discovery descriptor", error))?;
    DiscoveryDescriptor::from_json(&encoded)
  }

  #[cfg(not(any(unix, windows)))]
  pub fn read(&self) -> Result<DiscoveryDescriptor, DiscoveryError> {
    Err(DiscoveryError::UnsupportedSecurityBoundary {
      platform: std::env::consts::OS,
    })
  }
}

#[cfg(unix)]
pub(crate) fn validate_private_file(
  path: &Path,
  metadata: &std::fs::Metadata,
) -> Result<(), DiscoveryError> {
  use std::os::unix::fs::MetadataExt;

  if !metadata.is_file() {
    return Err(DiscoveryError::WrongFileType(path.to_path_buf()));
  }
  if metadata.uid() != rustix::process::geteuid().as_raw() {
    return Err(DiscoveryError::WrongOwner {
      path: path.to_path_buf(),
    });
  }
  let actual = metadata.mode() & 0o777;
  if actual != 0o600 {
    return Err(DiscoveryError::InsecurePermissions {
      path: path.to_path_buf(),
      expected: 0o600,
      actual,
    });
  }
  Ok(())
}
