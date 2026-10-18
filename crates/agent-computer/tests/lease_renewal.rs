mod support;

use std::{
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use agent_computer::*;
use support::*;
use uuid::Uuid;

#[test]
fn repeated_renewal_preserves_token_beyond_five_minutes_and_checks_ttl() {
  let f = Fixture::new();
  let lease = f.lease();
  for _ in 0..4 {
    f.clock.advance(20_000);
    f.runtime
      .renew_lease(&lease, Duration::from_secs(300))
      .expect("renew");
  }
  f.clock.advance(240_000);
  assert_eq!(f.leases.validate(&lease), Ok(()));
  for ttl in [
    Duration::ZERO,
    Duration::from_millis(999),
    Duration::from_millis(300_001),
  ] {
    assert!(matches!(
      f.leases.renew(&lease, ttl),
      Err(DesktopError::InvalidRequest(_))
    ));
  }
  // Rejected TTLs leave the previous deadline intact.
  f.clock.advance(60_000);
  assert_eq!(f.leases.validate(&lease), Err(DesktopError::LeaseInvalid));
  let effects = f.effects.lock().expect("effects");
  assert_eq!(effects.calls, 0);
  assert!(effects.observed_limits.is_empty());
}

#[test]
fn expired_revoked_and_replaced_tokens_cannot_renew() {
  let f = Fixture::new();
  let old = f.lease();
  f.clock.advance(30_000);
  assert_eq!(
    f.leases.renew(&old, Duration::from_secs(300)),
    Err(DesktopError::LeaseInvalid)
  );
  let current = f.lease();
  assert_eq!(
    f.leases.renew(&old, Duration::from_secs(300)),
    Err(DesktopError::LeaseInvalid)
  );
  f.leases.revoke_token(&current).expect("revoke");
  assert_eq!(
    f.leases.renew(&current, Duration::from_secs(300)),
    Err(DesktopError::LeaseInvalid)
  );
  let replacement = f.lease();
  let mut forged = replacement.clone();
  forged.session_id = Uuid::new_v4();
  assert_eq!(
    f.leases.renew(&forged, Duration::from_secs(300)),
    Err(DesktopError::LeaseInvalid)
  );
  f.clock.advance(30_000);
  assert_eq!(
    f.leases.validate(&replacement),
    Err(DesktopError::LeaseInvalid)
  );
}

struct PermissionBackend {
  inner: Backend,
  denied: Arc<AtomicBool>,
}

impl DesktopBackend for PermissionBackend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    let mut capabilities = self.inner.capabilities()?;
    if self.denied.load(Ordering::SeqCst) {
      capabilities.input = PermissionState::Denied;
    }
    Ok(capabilities)
  }
  fn observe(
    &mut self,
    session: &DesktopSession,
    limits: ObservationLimits,
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    self.inner.observe(session, limits, control)
  }
  fn validate_snapshot(&mut self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
    self.inner.validate_snapshot(snapshot)
  }
  fn execute(
    &mut self,
    action: &DesktopAction,
    snapshot: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome {
    self.inner.execute(action, snapshot, guard)
  }
}

#[test]
fn renewal_requires_current_permission_and_runtime_session_without_os_effects() {
  let f = Fixture::new();
  let denied = Arc::new(AtomicBool::new(false));
  let runtime = DesktopRuntime::new(
    f.session.clone(),
    f.leases.clone(),
    Box::new(PermissionBackend {
      inner: Backend {
        session: f.session.clone(),
        effects: f.effects.clone(),
      },
      denied: denied.clone(),
    }),
  );
  let lease = runtime
    .acquire_lease(Duration::from_secs(1))
    .expect("lease");
  denied.store(true, Ordering::SeqCst);
  assert!(matches!(
    runtime.renew_lease(&lease, Duration::from_secs(300)),
    Err(DesktopError::PermissionDenied(_))
  ));
  denied.store(false, Ordering::SeqCst);
  let mut wrong_session = lease.clone();
  wrong_session.session_id = Uuid::new_v4();
  assert_eq!(
    runtime.renew_lease(&wrong_session, Duration::from_secs(300)),
    Err(DesktopError::LeaseInvalid)
  );
  f.clock.advance(1000);
  assert_eq!(f.leases.validate(&lease), Err(DesktopError::LeaseInvalid));
  let effects = f.effects.lock().expect("effects");
  assert_eq!(effects.calls, 0);
  assert!(effects.observed_limits.is_empty());
}

struct InFlightBackend {
  inner: Backend,
  leases: Arc<LeaseRegistry>,
  token: Arc<Mutex<Option<LeaseToken>>>,
}

impl DesktopBackend for InFlightBackend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    self.inner.capabilities()
  }
  fn observe(
    &mut self,
    session: &DesktopSession,
    limits: ObservationLimits,
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    let token = self.token.lock().expect("token");
    assert_eq!(
      self.leases.renew(
        token.as_ref().expect("live token"),
        Duration::from_secs(300)
      ),
      Err(DesktopError::LeaseBusy)
    );
    self.inner.observe(session, limits, control)
  }
  fn validate_snapshot(&mut self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
    self.inner.validate_snapshot(snapshot)
  }
  fn execute(
    &mut self,
    action: &DesktopAction,
    snapshot: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome {
    self.inner.execute(action, snapshot, guard)
  }
}

#[test]
fn renewal_during_in_flight_native_operation_does_not_extend_deadline() {
  let f = Fixture::new();
  let token = Arc::new(Mutex::new(None));
  let runtime = DesktopRuntime::new(
    f.session.clone(),
    f.leases.clone(),
    Box::new(InFlightBackend {
      inner: Backend {
        session: f.session.clone(),
        effects: f.effects.clone(),
      },
      leases: f.leases.clone(),
      token: token.clone(),
    }),
  );
  let lease = runtime
    .acquire_lease(Duration::from_secs(1))
    .expect("lease");
  *token.lock().expect("token") = Some(lease.clone());
  runtime
    .observe(&lease, ObservationLimits::default(), f.control())
    .expect("observe");
  f.clock.advance(1000);
  assert_eq!(f.leases.validate(&lease), Err(DesktopError::LeaseInvalid));
}
