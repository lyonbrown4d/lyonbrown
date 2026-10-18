use xcap::{Monitor, Window};

use super::{capture, platform};
use crate::*;

const MAX_TARGETS: usize = 512;
const MAX_LABEL_CHARACTERS: usize = 512;

/// Enumerate bounded metadata only, without capturing pixels or prompting.
/// An unavailable/locked/unknown session or missing capture grant fails closed.
/// Minimized and zero-sized targets are omitted; overfull catalogs are rejected.
pub fn list_targets() -> Result<Vec<DesktopTarget>, DesktopError> {
  inspect_catalog(platform::session, platform::capture_permission, catalog)
}

fn inspect_catalog(
  mut session: impl FnMut() -> Result<String, DesktopError>,
  mut permission: impl FnMut() -> PermissionState,
  enumerate: impl FnOnce() -> Result<Vec<DesktopTarget>, DesktopError>,
) -> Result<Vec<DesktopTarget>, DesktopError> {
  let before = session()?;
  check_permission(permission())?;
  let targets = enumerate()?;
  if session()? != before {
    return Err(DesktopError::SessionUnavailable);
  }
  check_permission(permission())?;
  Ok(targets)
}

fn check_permission(permission: PermissionState) -> Result<(), DesktopError> {
  if permission != PermissionState::Granted {
    return Err(DesktopError::PermissionDenied("screen capture".into()));
  }
  Ok(())
}

fn catalog() -> Result<Vec<DesktopTarget>, DesktopError> {
  let mut targets = Vec::new();
  // xcap exposes whole-list enumeration; bound the resulting application catalog
  // and stop processing native metadata as soon as that bound is exceeded.
  for monitor in Monitor::all().map_err(capture::failure)? {
    let bounds = capture::monitor_bounds(&monitor)?;
    if !available(bounds) {
      continue;
    }
    let target = DesktopTarget {
      scope: TargetScope::Display(monitor.id().map_err(capture::failure)?),
      label: monitor.name().map_err(capture::failure)?,
      process_id: None,
      bounds,
    };
    push_target(&mut targets, target)?;
  }
  for window in Window::all().map_err(capture::failure)? {
    if window.is_minimized().map_err(capture::failure)? {
      continue;
    }
    let bounds = window_bounds(&window)?;
    if !available(bounds) {
      continue;
    }
    let process_id = window.pid().map_err(capture::failure)?;
    if process_id == 0 {
      continue;
    }
    let target = DesktopTarget {
      scope: TargetScope::Window(window.id().map_err(capture::failure)?),
      label: window.title().map_err(capture::failure)?,
      process_id: Some(process_id),
      bounds,
    };
    push_target(&mut targets, target)?;
  }
  Ok(targets)
}

fn push_target(
  targets: &mut Vec<DesktopTarget>,
  mut target: DesktopTarget,
) -> Result<(), DesktopError> {
  if !available(target.bounds) {
    return Ok(());
  }
  if targets.len() >= MAX_TARGETS {
    return Err(DesktopError::CapacityExceeded);
  }
  target.label = target
    .label
    .chars()
    .filter(|character| !character.is_control())
    .take(MAX_LABEL_CHARACTERS)
    .collect();
  targets.push(target);
  Ok(())
}

fn available(bounds: Bounds) -> bool {
  bounds.width > 0 && bounds.height > 0
}

fn window_bounds(window: &Window) -> Result<Bounds, DesktopError> {
  Ok(Bounds {
    x: window.x().map_err(capture::failure)?,
    y: window.y().map_err(capture::failure)?,
    width: window.width().map_err(capture::failure)?,
    height: window.height().map_err(capture::failure)?,
  })
}

/// Stable attachment scope; titles and geometry are checked against each fresh
/// snapshot, rather than against the older metadata shown in the target picker.
pub(super) struct TargetBinding {
  scope: TargetScope,
  process_id: Option<u32>,
}

impl TargetBinding {
  pub(super) fn new(target: &DesktopTarget) -> Result<Self, DesktopError> {
    if !available(target.bounds)
      || !matches!(
        (&target.scope, target.process_id),
        (TargetScope::Display(_), None) | (TargetScope::Window(_), Some(1..))
      )
    {
      return Err(DesktopError::InvalidRequest("native desktop target".into()));
    }
    Ok(Self {
      scope: target.scope.clone(),
      process_id: target.process_id,
    })
  }

  pub(super) fn check_scope(&self, scope: &TargetScope) -> Result<(), DesktopError> {
    if *scope != self.scope {
      return Err(DesktopError::StaleSnapshot);
    }
    Ok(())
  }

  pub(super) fn check_identity(
    &self,
    scope: &TargetScope,
    process_id: Option<u32>,
  ) -> Result<(), DesktopError> {
    self.check_scope(scope)?;
    if process_id != self.process_id {
      return Err(DesktopError::StaleSnapshot);
    }
    Ok(())
  }
}

pub(super) fn validate(target: &TargetBinding) -> Result<(), DesktopError> {
  match target.scope {
    TargetScope::Display(id) => {
      let monitor = capture::monitor(id)?;
      if !available(capture::monitor_bounds(&monitor)?) {
        return Err(DesktopError::SessionUnavailable);
      }
      target.check_identity(
        &TargetScope::Display(monitor.id().map_err(capture::failure)?),
        None,
      )
    }
    TargetScope::Window(id) => {
      let window = capture::window(id)?;
      if window.is_minimized().map_err(capture::failure)? || !available(window_bounds(&window)?) {
        return Err(DesktopError::SessionUnavailable);
      }
      target.check_identity(
        &TargetScope::Window(window.id().map_err(capture::failure)?),
        Some(window.pid().map_err(capture::failure)?),
      )
    }
  }
}

#[cfg(test)]
#[path = "../../tests/support/native_targets.rs"]
mod tests;
