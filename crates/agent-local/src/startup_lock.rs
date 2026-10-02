use crate::{DiscoveryError, SecureRunDirectory, file_lock::FileLock};

const LOCK_FILE: &str = "agentd.start.lock";

#[derive(Debug)]
pub struct StartupLock {
  _lock: FileLock,
}

impl StartupLock {
  pub fn try_acquire(run_dir: &SecureRunDirectory) -> Result<Option<Self>, DiscoveryError> {
    FileLock::try_acquire(run_dir, LOCK_FILE, "acquiring startup lock")
      .map(|lock| lock.map(|_lock| Self { _lock }))
  }
}
