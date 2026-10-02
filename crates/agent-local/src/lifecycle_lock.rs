use crate::{DiscoveryError, SecureRunDirectory, file_lock::FileLock};

const LOCK_FILE: &str = "agentd.lock";

#[derive(Debug)]
pub struct LifecycleLock {
  _lock: FileLock,
}

impl LifecycleLock {
  pub fn try_acquire(run_dir: &SecureRunDirectory) -> Result<Option<Self>, DiscoveryError> {
    FileLock::try_acquire(run_dir, LOCK_FILE, "acquiring lifecycle lock")
      .map(|lock| lock.map(|_lock| Self { _lock }))
  }
}
