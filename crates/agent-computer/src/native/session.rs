use std::sync::atomic::{AtomicBool, Ordering};

use crate::DesktopError;

/// A native attachment cannot regain authority after observing a lock, logout,
/// switch or unreadable session state. A new explicit attachment is required.
#[derive(Default)]
pub(super) struct SessionGuard {
  revoked: AtomicBool,
}

impl SessionGuard {
  pub(super) fn check(
    &self,
    expected: &str,
    inspect: impl FnOnce() -> Result<String, DesktopError>,
  ) -> Result<(), DesktopError> {
    if self.revoked.load(Ordering::Acquire) {
      return Err(DesktopError::SessionUnavailable);
    }
    let result = inspect().and_then(|current| {
      if current == expected {
        Ok(())
      } else {
        Err(DesktopError::SessionUnavailable)
      }
    });
    if result.is_err() {
      self.revoked.store(true, Ordering::Release);
    }
    result
  }
}

#[cfg(test)]
#[path = "../../tests/support/native_session.rs"]
mod tests;
