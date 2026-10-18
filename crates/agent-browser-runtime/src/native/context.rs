use super::handlers::ContextCallbacks;
use agent_browser::{BrowserError, BrowserResult, ProfileLease, Space};
use cef::*;
use std::{
  cell::Cell,
  path::Path,
  rc::Rc,
  time::{Duration, Instant},
};

pub(super) struct SpaceRecord {
  pub space: Space,
  pub context: RequestContext,
  _lease: ProfileLease,
  ready: Rc<Cell<bool>>,
}

impl SpaceRecord {
  pub fn new(
    root: &Path,
    session_id: String,
    profile_id: String,
    headless: bool,
  ) -> BrowserResult<Self> {
    if session_id.is_empty() {
      return Err(BrowserError::InvalidInput("session id is empty".into()));
    }
    let lease = ProfileLease::acquire(root, &profile_id)?;
    let settings = RequestContextSettings {
      cache_path: lease.directory().to_string_lossy().as_ref().into(),
      persist_session_cookies: 1,
      ..Default::default()
    };
    let ready = Rc::new(Cell::new(false));
    let mut handler = ContextCallbacks::new(ready.clone());
    let context = request_context_create_context(Some(&settings), Some(&mut handler))
      .ok_or_else(|| BrowserError::Unavailable("CEF request context creation failed".into()))?;
    Ok(Self {
      space: Space {
        id: uuid::Uuid::now_v7().to_string(),
        session_id,
        profile_id,
        headless,
        human_control: false,
      },
      context,
      _lease: lease,
      ready,
    })
  }

  pub fn wait_ready(&self) -> BrowserResult<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !self.ready.get() {
      crate::native::pump();
      if Instant::now() >= deadline {
        // The caller retains the record and profile lease on an unknown result.
        return Err(BrowserError::Uncertain);
      }
      std::thread::sleep(Duration::from_millis(4));
    }
    Ok(())
  }
}
