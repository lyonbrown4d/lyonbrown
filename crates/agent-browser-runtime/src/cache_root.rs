use agent_browser::{BrowserError, BrowserResult};
use std::{
  fs::{File, OpenOptions},
  path::Path,
};

/// Holds exclusive ownership of one persistent CEF installation root.
///
/// Keep this lease until CEF has shut down. Profile leases alone do not protect
/// CEF's installation data, which is shared by every profile beneath the root.
pub struct CacheRootLease {
  _file: File,
}

impl CacheRootLease {
  pub fn acquire(root: &Path) -> BrowserResult<Self> {
    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(root.join("lyonbrown-runtime.lock"))
      .map_err(|error| BrowserError::Unavailable(format!("browser runtime storage: {error}")))?;
    file.try_lock().map_err(|error| {
      BrowserError::Unavailable(format!(
        "browser runtime cache root is unavailable or already owned by another client process: {error}"
      ))
    })?;
    Ok(Self { _file: file })
  }
}
