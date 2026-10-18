mod support;

use std::time::Duration;

use agent_computer::*;
use support::*;
use uuid::Uuid;

#[test]
fn approved_action_consumes_snapshot_and_receipt_prevents_replay() {
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  let request = request(&lease, &snapshot);
  let action_id = request.action_id;
  let receipt = f
    .runtime
    .execute(approved(request.clone()), f.control())
    .expect("receipt");
  assert_eq!(receipt.outcome, ActionOutcome::Completed);
  assert_eq!(f.runtime.receipt(action_id).expect("stored"), Some(receipt));
  assert_eq!(
    f.runtime.execute(approved(request.clone()), f.control()),
    Err(DesktopError::DuplicateAction)
  );
  let mut next = request;
  next.action_id = Uuid::new_v4();
  assert_eq!(
    f.runtime.execute(approved(next), f.control()),
    Err(DesktopError::StaleSnapshot)
  );
  assert_eq!(f.effects.lock().expect("effects").calls, 1);
}

#[test]
fn changed_snapshot_revision_scope_and_age_reject_before_input() {
  let f = Fixture::new();
  let lease = f.lease();
  let previous = f.snapshot(&lease);
  let current = f.snapshot(&lease);
  assert_eq!(
    f.runtime
      .execute(approved(request(&lease, &previous)), f.control()),
    Err(DesktopError::StaleSnapshot)
  );
  let mut mismatched = request(&lease, &current);
  mismatched.snapshot_revision += 1;
  assert_eq!(
    f.runtime.execute(approved(mismatched), f.control()),
    Err(DesktopError::StaleSnapshot)
  );
  let mut outside = request(&lease, &current);
  outside.action = DesktopAction::Click {
    x: 2,
    y: 1,
    button: MouseButton::Left,
  };
  assert!(matches!(
    f.runtime.execute(approved(outside), f.control()),
    Err(DesktopError::InvalidRequest(_))
  ));
  f.clock.advance(2000);
  assert_eq!(
    f.runtime
      .execute(approved(request(&lease, &current)), f.control()),
    Err(DesktopError::StaleSnapshot)
  );
  assert_eq!(f.effects.lock().expect("effects").calls, 0);
}

#[test]
fn permission_lock_and_focus_failures_are_receipts_without_input() {
  for error in [
    DesktopError::PermissionDenied("input".into()),
    DesktopError::Locked,
    DesktopError::SessionUnavailable,
    DesktopError::SessionStateUnknown,
    DesktopError::StaleSnapshot,
  ] {
    let f = Fixture::new();
    let lease = f.lease();
    let snapshot = f.snapshot(&lease);
    f.effects.lock().expect("effects").validate_error = Some(error.clone());
    let request = request(&lease, &snapshot);
    let id = request.action_id;
    let receipt = f
      .runtime
      .execute(approved(request), f.control())
      .expect("receipt");
    assert_eq!(receipt.outcome, ActionOutcome::Rejected(error));
    assert_eq!(f.runtime.receipt(id).expect("stored"), Some(receipt));
    assert_eq!(f.effects.lock().expect("effects").calls, 0);
  }
}

#[test]
fn revocation_and_expiry_invalidate_the_old_controller() {
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  f.runtime.revoke().expect("revoke");
  let replacement = f.lease();
  assert_ne!(lease.id, replacement.id);
  assert_eq!(
    f.runtime
      .execute(approved(request(&lease, &snapshot)), f.control()),
    Err(DesktopError::LeaseInvalid)
  );
  assert!(matches!(
    f.runtime
      .observe(&lease, ObservationLimits::default(), f.control()),
    Err(DesktopError::LeaseInvalid)
  ));
  f.clock.advance(30_000);
  assert_eq!(
    f.leases.validate(&replacement),
    Err(DesktopError::LeaseInvalid)
  );
  assert!(f.runtime.acquire_lease(Duration::from_secs(1)).is_ok());
  assert_eq!(f.effects.lock().expect("effects").calls, 0);
}

#[test]
fn cancellation_and_deadline_before_dispatch_have_no_side_effects() {
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  let control = f.control();
  control.cancellation.cancel();
  assert_eq!(
    f.runtime
      .execute(approved(request(&lease, &snapshot)), control),
    Err(DesktopError::Cancelled)
  );
  let mut control = f.control();
  control.deadline = f.clock.now();
  assert_eq!(
    f.runtime
      .execute(approved(request(&lease, &snapshot)), control),
    Err(DesktopError::DeadlineExceeded)
  );
  assert_eq!(f.effects.lock().expect("effects").calls, 0);
}

#[test]
fn unknown_or_cancelled_dispatched_input_is_never_replayed() {
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  let control = f.control();
  f.effects.lock().expect("effects").cancel_during_input = Some(control.cancellation.clone());
  let original = request(&lease, &snapshot);
  let id = original.action_id;
  let receipt = f
    .runtime
    .execute(approved(original.clone()), control)
    .expect("receipt");
  assert_eq!(
    receipt.outcome,
    ActionOutcome::Uncertain(DesktopError::Cancelled)
  );
  assert_eq!(f.runtime.receipt(id).expect("stored"), Some(receipt));
  let fresh = f.snapshot(&lease);
  let mut retry = request(&lease, &fresh);
  retry.action_id = original.action_id;
  assert_eq!(
    f.runtime.execute(approved(retry), f.control()),
    Err(DesktopError::DuplicateAction)
  );
  assert_eq!(f.effects.lock().expect("effects").calls, 1);
}

#[test]
fn observation_limits_validate_bytes_and_preserve_last_valid_snapshot() {
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  f.effects.lock().expect("effects").invalid_image = true;
  assert!(matches!(
    f.runtime
      .observe(&lease, ObservationLimits::default(), f.control()),
    Err(DesktopError::CapacityExceeded)
  ));
  let limits = ObservationLimits {
    max_pixels: 3,
    ..ObservationLimits::default()
  };
  assert!(matches!(
    f.runtime.observe(&lease, limits, f.control()),
    Err(DesktopError::CapacityExceeded)
  ));
  let limits = ObservationLimits {
    max_nodes: 129,
    ..ObservationLimits::default()
  };
  assert!(matches!(
    f.runtime.observe(&lease, limits, f.control()),
    Err(DesktopError::InvalidRequest(_))
  ));
  // A failed refresh cannot mint an actionable snapshot; the original remains
  // subject to its existing age/lease and platform validation checks.
  assert_eq!(
    f.runtime
      .execute(approved(request(&lease, &snapshot)), f.control())
      .expect("receipt")
      .outcome,
    ActionOutcome::Completed
  );
}

#[test]
fn slow_capture_cannot_refresh_the_age_of_an_old_screenshot() {
  let f = Fixture::new();
  let lease = f.lease();
  f.effects.lock().expect("effects").advance_during_capture = Some((f.clock.clone(), 2000));
  assert!(matches!(
    f.runtime
      .observe(&lease, ObservationLimits::default(), f.control()),
    Err(DesktopError::StaleSnapshot)
  ));
  assert_eq!(f.effects.lock().expect("effects").calls, 0);
}
