use crate::{DiscoverySource, StartupGuard};
use agent_local::{
  DiscoveryDescriptor, DiscoveryError, DiscoveryStore, SecureRunDirectory, StartupLock,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalDiscoverySource {
  run_directory: PathBuf,
}

impl LocalDiscoverySource {
  pub fn new(run_directory: impl AsRef<Path>) -> Self {
    Self {
      run_directory: run_directory.as_ref().to_path_buf(),
    }
  }

  pub fn run_directory(&self) -> &Path {
    &self.run_directory
  }

  fn open(&self) -> Result<SecureRunDirectory, DiscoveryError> {
    SecureRunDirectory::open(&self.run_directory)
  }
}

struct LocalStartupGuard {
  _lock: StartupLock,
}

impl StartupGuard for LocalStartupGuard {}

impl DiscoverySource for LocalDiscoverySource {
  fn read(&self) -> Result<Option<DiscoveryDescriptor>, DiscoveryError> {
    let store = DiscoveryStore::new(self.open()?);
    match store.read() {
      Ok(descriptor) => Ok(Some(descriptor)),
      Err(DiscoveryError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
        Ok(None)
      }
      Err(error) => Err(error),
    }
  }

  fn try_acquire_startup_lock(&self) -> Result<Option<Box<dyn StartupGuard>>, DiscoveryError> {
    let run_directory = self.open()?;
    let lock = StartupLock::try_acquire(&run_directory)?;
    Ok(lock.map(|lock| Box::new(LocalStartupGuard { _lock: lock }) as Box<dyn StartupGuard>))
  }
}
