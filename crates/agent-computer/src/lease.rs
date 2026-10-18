use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{DesktopError, DesktopIdentity, DesktopSession};

pub trait DesktopClock: Send + Sync {
  fn now(&self) -> Instant;
}

pub struct SystemDesktopClock;

impl DesktopClock for SystemDesktopClock {
  fn now(&self) -> Instant {
    Instant::now()
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LeaseToken {
  pub id: Uuid,
  pub session_id: Uuid,
  pub identity: DesktopIdentity,
}

struct ActiveLease {
  token: LeaseToken,
  expires: Instant,
  revoked: CancellationToken,
}

#[derive(Default)]
struct Entry {
  active: Option<ActiveLease>,
  in_flight: bool,
}

/// The interactive helper must share one registry for all requests. Exclusivity
/// is keyed by OS desktop identity, including requests for different windows.
/// Cross-process exclusion/authentication is the helper launcher's responsibility.
pub struct LeaseRegistry {
  entries: Mutex<HashMap<DesktopIdentity, Entry>>,
  clock: Arc<dyn DesktopClock>,
}

impl Default for LeaseRegistry {
  fn default() -> Self {
    Self::new(Arc::new(SystemDesktopClock))
  }
}

impl LeaseRegistry {
  pub fn new(clock: Arc<dyn DesktopClock>) -> Self {
    Self {
      entries: Mutex::new(HashMap::new()),
      clock,
    }
  }

  pub fn acquire(
    &self,
    session: &DesktopSession,
    ttl: Duration,
  ) -> Result<LeaseToken, DesktopError> {
    if ttl.is_zero() || ttl > Duration::from_secs(300) {
      return Err(DesktopError::InvalidRequest(
        "lease TTL must be within 300 seconds".into(),
      ));
    }
    let mut entries = self
      .entries
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    if entries.len() >= 256 && !entries.contains_key(&session.identity) {
      return Err(DesktopError::CapacityExceeded);
    }
    let entry = entries.entry(session.identity.clone()).or_default();
    let now = self.clock.now();
    if entry.in_flight
      || entry
        .active
        .as_ref()
        .is_some_and(|a| now < a.expires && !a.revoked.is_cancelled())
    {
      return Err(DesktopError::LeaseBusy);
    }
    if let Some(old) = &entry.active {
      old.revoked.cancel();
    }
    let token = LeaseToken {
      id: Uuid::new_v4(),
      session_id: session.id,
      identity: session.identity.clone(),
    };
    entry.active = Some(ActiveLease {
      token: token.clone(),
      expires: now + ttl,
      revoked: CancellationToken::new(),
    });
    Ok(token)
  }

  /// Called by trusted human-takeover/session-lock handling. Does not wait for a
  /// blocking OS call: the old controller is invalid immediately. Acquisition is
  /// held off until any already-dispatched OS step has drained.
  pub fn revoke(&self, identity: &DesktopIdentity) -> Result<(), DesktopError> {
    let entries = self
      .entries
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    if let Some(active) = entries.get(identity).and_then(|e| e.active.as_ref()) {
      active.revoked.cancel();
    }
    Ok(())
  }

  pub fn validate(&self, token: &LeaseToken) -> Result<(), DesktopError> {
    let entries = self
      .entries
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    let active = entries
      .get(&token.identity)
      .and_then(|e| e.active.as_ref())
      .ok_or(DesktopError::LeaseInvalid)?;
    if active.token != *token || active.revoked.is_cancelled() || self.clock.now() >= active.expires
    {
      return Err(DesktopError::LeaseInvalid);
    }
    Ok(())
  }

  /// Revoke only this live token. Validation and cancellation share the lock so
  /// an expired controller cannot revoke a replacement controller's lease.
  pub fn revoke_token(&self, token: &LeaseToken) -> Result<(), DesktopError> {
    let entries = self
      .entries
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    let active = entries
      .get(&token.identity)
      .and_then(|entry| entry.active.as_ref())
      .ok_or(DesktopError::LeaseInvalid)?;
    if active.token != *token || active.revoked.is_cancelled() || self.clock.now() >= active.expires
    {
      return Err(DesktopError::LeaseInvalid);
    }
    active.revoked.cancel();
    Ok(())
  }

  pub(crate) fn begin(
    self: &Arc<Self>,
    token: &LeaseToken,
  ) -> Result<LeaseExecution, DesktopError> {
    let mut entries = self
      .entries
      .lock()
      .map_err(|_| DesktopError::CoordinationUnavailable)?;
    let entry = entries
      .get_mut(&token.identity)
      .ok_or(DesktopError::LeaseInvalid)?;
    let active = entry.active.as_ref().ok_or(DesktopError::LeaseInvalid)?;
    if active.token != *token || active.revoked.is_cancelled() || self.clock.now() >= active.expires
    {
      return Err(DesktopError::LeaseInvalid);
    }
    if entry.in_flight {
      return Err(DesktopError::LeaseBusy);
    }
    entry.in_flight = true;
    Ok(LeaseExecution {
      registry: Arc::clone(self),
      token: token.clone(),
    })
  }

  pub(crate) fn now(&self) -> Instant {
    self.clock.now()
  }
}

pub(crate) struct LeaseExecution {
  pub registry: Arc<LeaseRegistry>,
  pub token: LeaseToken,
}

impl Drop for LeaseExecution {
  fn drop(&mut self) {
    if let Ok(mut entries) = self.registry.entries.lock()
      && let Some(entry) = entries.get_mut(&self.token.identity)
    {
      entry.in_flight = false;
    }
  }
}
