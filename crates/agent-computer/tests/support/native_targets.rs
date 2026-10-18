use std::cell::Cell;

use super::*;

fn target(scope: TargetScope, process_id: Option<u32>) -> DesktopTarget {
  DesktopTarget {
    scope,
    process_id,
    label: "private document title".into(),
    bounds: Bounds {
      x: 0,
      y: 0,
      width: 200,
      height: 100,
    },
  }
}

#[test]
fn catalog_denies_unknown_session_and_missing_permission_before_enumeration() {
  for failure in [DesktopError::Locked, DesktopError::SessionStateUnknown] {
    let enumerated = Cell::new(false);
    let result = inspect_catalog(
      || Err(failure.clone()),
      || PermissionState::Granted,
      || {
        enumerated.set(true);
        Ok(vec![])
      },
    );
    assert_eq!(result, Err(failure));
    assert!(!enumerated.get());
  }
  let enumerated = Cell::new(false);
  let result = inspect_catalog(
    || Ok("session".into()),
    || PermissionState::Denied,
    || {
      enumerated.set(true);
      Ok(vec![])
    },
  );
  assert_eq!(
    result,
    Err(DesktopError::PermissionDenied("screen capture".into()))
  );
  assert!(!enumerated.get());
}

#[test]
fn catalog_returns_metadata_only_while_session_and_permission_remain_valid() {
  let expected = vec![target(TargetScope::Window(42), Some(7))];
  let result = inspect_catalog(
    || Ok("session".into()),
    || PermissionState::Granted,
    || Ok(expected.clone()),
  );
  assert_eq!(result, Ok(expected));

  let changed = Cell::new(false);
  let result = inspect_catalog(
    || Ok(if changed.get() { "other" } else { "session" }.into()),
    || PermissionState::Granted,
    || {
      changed.set(true);
      Ok(vec![target(TargetScope::Display(1), None)])
    },
  );
  assert_eq!(result, Err(DesktopError::SessionUnavailable));

  let revoked = Cell::new(false);
  let result = inspect_catalog(
    || Ok("session".into()),
    || {
      if revoked.get() {
        PermissionState::Denied
      } else {
        PermissionState::Granted
      }
    },
    || {
      revoked.set(true);
      Ok(vec![target(TargetScope::Display(1), None)])
    },
  );
  assert_eq!(
    result,
    Err(DesktopError::PermissionDenied("screen capture".into()))
  );
}

#[test]
fn catalog_cleans_and_bounds_private_labels_and_omits_zero_sized_targets() {
  let mut targets = Vec::new();
  let mut item = target(TargetScope::Window(42), Some(7));
  item.label = format!("\0\n\t{}\u{7f}", "中".repeat(600));
  push_target(&mut targets, item).expect("bounded label");
  assert_eq!(targets[0].label, "中".repeat(MAX_LABEL_CHARACTERS));
  assert!(!format!("{:?}", targets[0]).contains('中'));
  let mut item = target(TargetScope::Display(1), None);
  item.bounds.width = 0;
  push_target(&mut targets, item).expect("unavailable target omitted");
  assert_eq!(targets.len(), 1);
}

#[test]
fn catalog_accepts_exact_capacity_and_rejects_overflow() {
  let mut targets = Vec::new();
  for id in 0..MAX_TARGETS as u32 {
    push_target(&mut targets, target(TargetScope::Window(id), Some(7))).expect("capacity");
  }
  assert_eq!(targets.len(), MAX_TARGETS);
  assert_eq!(
    push_target(&mut targets, target(TargetScope::Display(1), None)),
    Err(DesktopError::CapacityExceeded)
  );
  assert_eq!(targets.len(), MAX_TARGETS);
}

#[test]
fn selected_window_rejects_id_reuse_and_scope_expansion_but_allows_title_changes() {
  let selected = target(TargetScope::Window(42), Some(7));
  let binding = TargetBinding::new(&selected).expect("selected window");
  let mut current = selected;
  current.label = "title after approved input".into();
  current.bounds.x = 50;
  assert_eq!(
    binding.check_identity(&current.scope, current.process_id),
    Ok(())
  );
  assert_eq!(
    binding.check_identity(&TargetScope::Window(42), Some(8)),
    Err(DesktopError::StaleSnapshot)
  );
  assert_eq!(
    binding.check_identity(&TargetScope::Window(43), Some(7)),
    Err(DesktopError::StaleSnapshot)
  );
  assert_eq!(
    binding.check_scope(&TargetScope::Display(42)),
    Err(DesktopError::StaleSnapshot)
  );
  let display =
    TargetBinding::new(&target(TargetScope::Display(1), None)).expect("selected display");
  assert_eq!(
    display.check_identity(&TargetScope::Display(1), None),
    Ok(())
  );
  assert_eq!(
    display.check_scope(&TargetScope::Display(2)),
    Err(DesktopError::StaleSnapshot)
  );
}

#[test]
fn selected_target_requires_unambiguous_process_identity_and_available_bounds() {
  for item in [
    target(TargetScope::Window(42), None),
    target(TargetScope::Window(42), Some(0)),
    target(TargetScope::Display(1), Some(7)),
    DesktopTarget {
      bounds: Bounds {
        width: 0,
        height: 0,
        x: 0,
        y: 0,
      },
      ..target(TargetScope::Window(42), Some(7))
    },
  ] {
    assert!(matches!(
      TargetBinding::new(&item),
      Err(DesktopError::InvalidRequest(_))
    ));
  }
}
