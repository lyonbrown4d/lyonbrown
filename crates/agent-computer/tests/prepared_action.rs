mod support;

use std::time::Duration;

use agent_computer::*;
use support::*;
use uuid::Uuid;

#[test]
fn delayed_approval_refreshes_only_snapshot_identity_with_original_limits() {
  let f = Fixture::new();
  let lease = f.lease();
  let limits = ObservationLimits {
    max_pixels: 4,
    max_bytes: 16,
    max_windows: 1,
    max_nodes: 0,
    max_depth: 0,
  };
  let snapshot = f
    .runtime
    .observe(&lease, limits, f.control())
    .expect("capture");
  let original = request(&lease, &snapshot);
  let prepared = f.runtime.prepare_action(original.clone()).expect("prepare");
  assert!(prepared.request() == &original);
  let digest = *prepared.content_digest();
  f.clock.advance(2500);
  let refreshed = f
    .runtime
    .refresh_prepared_action(&prepared, f.control())
    .expect("refresh");
  assert_eq!(refreshed.action_id, original.action_id);
  assert_eq!(refreshed.lease, original.lease);
  assert!(refreshed.action == original.action);
  assert_ne!(refreshed.snapshot_id, original.snapshot_id);
  assert!(refreshed.snapshot_revision > original.snapshot_revision);
  let next_prepared = f
    .runtime
    .prepare_action(refreshed.clone())
    .expect("fresh prepare");
  assert_eq!(next_prepared.content_digest(), &digest);
  let observed = f.effects.lock().expect("effects");
  assert_eq!(observed.observed_limits.len(), 2);
  for actual in &observed.observed_limits {
    assert_eq!(
      (
        actual.max_pixels,
        actual.max_bytes,
        actual.max_windows,
        actual.max_nodes,
        actual.max_depth
      ),
      (4, 16, 1, 0, 0)
    );
  }
  drop(observed);
  assert_eq!(
    f.runtime.execute(approved(original), f.control()),
    Err(DesktopError::StaleSnapshot)
  );
  assert_eq!(
    f.runtime
      .execute(approved(refreshed), f.control())
      .expect("receipt")
      .outcome,
    ActionOutcome::Completed
  );
  assert_eq!(f.effects.lock().expect("effects").calls, 1);
  assert!(matches!(
    f.runtime.refresh_prepared_action(&prepared, f.control()),
    Err(DesktopError::DuplicateAction)
  ));
}

#[test]
fn changed_pixels_and_accessibility_content_reject_without_input() {
  for change in 0..8 {
    let f = Fixture::new();
    let lease = f.lease();
    let snapshot = f.snapshot(&lease);
    let prepared = f
      .runtime
      .prepare_action(request(&lease, &snapshot))
      .expect("prepare");
    let mut observation = snapshot.observation.clone();
    match change {
      0 => observation.screenshot.rgba[0] = 1,
      1 => observation.accessibility.nodes.push(AccessibilityNode {
        index: 0,
        parent: None,
        role: "button".into(),
        label: "Delete account".into(),
      }),
      2 => observation.accessibility.state = PermissionState::Denied,
      3 => observation.accessibility.truncated = true,
      4 => observation.windows_truncated = true,
      5 => observation.windows.push(WindowObservation {
        id: 42,
        process_id: 7,
        title: "Different content".into(),
        bounds: observation.bounds,
        focused: true,
      }),
      6 => observation.bounds.x = 1,
      _ => {
        observation.screenshot.width = 1;
        observation.screenshot.height = 4;
      }
    }
    f.effects.lock().expect("effects").observation = Some(observation);
    f.clock.advance(2500);
    assert!(matches!(
      f.runtime.refresh_prepared_action(&prepared, f.control()),
      Err(DesktopError::StaleSnapshot)
    ));
    assert_eq!(f.effects.lock().expect("effects").calls, 0);
  }
}

#[test]
fn changed_accessibility_labels_and_window_focus_reject_at_same_geometry() {
  for change in 0..7 {
    let f = Fixture::new();
    let lease = f.lease();
    let mut observation = f.snapshot(&lease).observation;
    observation.windows = vec![WindowObservation {
      id: 42,
      process_id: 7,
      title: "Original".into(),
      bounds: observation.bounds,
      focused: true,
    }];
    observation.accessibility = AccessibilityObservation {
      state: PermissionState::Granted,
      nodes: vec![AccessibilityNode {
        index: 0,
        parent: None,
        role: "button".into(),
        label: "Save".into(),
      }],
      truncated: false,
    };
    f.effects.lock().expect("effects").observation = Some(observation.clone());
    let snapshot = f.snapshot(&lease);
    let prepared = f
      .runtime
      .prepare_action(request(&lease, &snapshot))
      .expect("prepare");
    match change {
      0 => observation.accessibility.nodes[0].label = "Delete".into(),
      1 => observation.accessibility.nodes[0].role = "link".into(),
      2 => observation.accessibility.nodes[0].index = 1,
      3 => observation.accessibility.nodes[0].parent = Some(1),
      4 => observation.windows[0].title = "Changed".into(),
      5 => observation.windows[0].focused = false,
      _ => observation.windows[0].process_id = 8,
    }
    f.effects.lock().expect("effects").observation = Some(observation);
    assert!(matches!(
      f.runtime.refresh_prepared_action(&prepared, f.control()),
      Err(DesktopError::StaleSnapshot)
    ));
    assert_eq!(f.effects.lock().expect("effects").calls, 0);
  }
}

#[test]
fn prepared_binding_cannot_outlive_lease_or_transfer_to_another_runtime_or_scope() {
  for expired in [false, true] {
    let f = Fixture::new();
    let lease = f.lease();
    let snapshot = f.snapshot(&lease);
    let prepared = f
      .runtime
      .prepare_action(request(&lease, &snapshot))
      .expect("prepare");
    if expired {
      f.clock.advance(30_000);
    } else {
      f.runtime.revoke().expect("revoke");
    }
    let _replacement = f.lease();
    assert!(matches!(
      f.runtime.refresh_prepared_action(&prepared, f.control()),
      Err(DesktopError::LeaseInvalid)
    ));
    assert_eq!(f.effects.lock().expect("effects").calls, 0);
  }
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  let prepared = f
    .runtime
    .prepare_action(request(&lease, &snapshot))
    .expect("prepare");
  for scope in [f.session.scope.clone(), TargetScope::Display(42)] {
    let session = DesktopSession {
      scope,
      ..f.session.clone()
    };
    let replacement = DesktopRuntime::new(
      session.clone(),
      f.leases.clone(),
      Box::new(Backend {
        session,
        effects: f.effects.clone(),
      }),
    );
    assert!(matches!(
      replacement.refresh_prepared_action(&prepared, f.control()),
      Err(DesktopError::StaleSnapshot)
    ));
  }
  assert_eq!(f.effects.lock().expect("effects").calls, 0);
}

#[test]
fn preparation_rejects_invalid_revision_scope_age_and_lease() {
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  let original = request(&lease, &snapshot);
  let mut revised = original.clone();
  revised.snapshot_revision += 1;
  assert!(matches!(
    f.runtime.prepare_action(revised),
    Err(DesktopError::StaleSnapshot)
  ));
  let mut outside = original.clone();
  outside.action = DesktopAction::Move { x: 2, y: 1 };
  assert!(matches!(
    f.runtime.prepare_action(outside),
    Err(DesktopError::InvalidRequest(_))
  ));
  let mut other_lease = original.clone();
  other_lease.lease.id = Uuid::new_v4();
  assert!(matches!(
    f.runtime.prepare_action(other_lease),
    Err(DesktopError::LeaseInvalid)
  ));
  f.clock.advance(2000);
  assert!(matches!(
    f.runtime.prepare_action(original),
    Err(DesktopError::StaleSnapshot)
  ));
  assert_eq!(f.effects.lock().expect("effects").calls, 0);
}

#[test]
fn cancelled_refresh_does_not_dispatch_or_replace_original_action() {
  let f = Fixture::new();
  let lease = f.lease();
  let snapshot = f.snapshot(&lease);
  let original = request(&lease, &snapshot);
  let prepared = f.runtime.prepare_action(original.clone()).expect("prepare");
  let control = f.control();
  control.cancellation.cancel();
  assert!(matches!(
    f.runtime.refresh_prepared_action(&prepared, control),
    Err(DesktopError::Cancelled)
  ));
  assert!(prepared.request() == &original);
  assert_eq!(f.effects.lock().expect("effects").calls, 0);
  assert_eq!(
    f.leases.acquire(&f.session, Duration::from_secs(1)),
    Err(DesktopError::LeaseBusy)
  );
}
