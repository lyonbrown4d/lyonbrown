use xcap::{Monitor, Window};

use crate::*;

pub(super) fn failure(_: impl std::fmt::Display) -> DesktopError {
  // Upstream errors may contain window names or other private desktop content.
  DesktopError::BackendUnavailable("native capture operation failed".into())
}

fn window_data(window: &Window) -> Result<WindowObservation, DesktopError> {
  Ok(WindowObservation {
    id: window.id().map_err(failure)?,
    process_id: window.pid().map_err(failure)?,
    title: window.title().map_err(failure)?.chars().take(512).collect(),
    bounds: Bounds {
      x: window.x().map_err(failure)?,
      y: window.y().map_err(failure)?,
      width: window.width().map_err(failure)?,
      height: window.height().map_err(failure)?,
    },
    focused: window.is_focused().map_err(failure)?,
  })
}

pub(super) fn monitor(id: u32) -> Result<Monitor, DesktopError> {
  Monitor::all()
    .map_err(failure)?
    .into_iter()
    .find(|m| m.id().ok() == Some(id))
    .ok_or(DesktopError::StaleSnapshot)
}

pub(super) fn monitor_bounds(monitor: &Monitor) -> Result<Bounds, DesktopError> {
  Ok(Bounds {
    x: monitor.x().map_err(failure)?,
    y: monitor.y().map_err(failure)?,
    width: monitor.width().map_err(failure)?,
    height: monitor.height().map_err(failure)?,
  })
}

pub(super) fn window(id: u32) -> Result<Window, DesktopError> {
  Window::all()
    .map_err(failure)?
    .into_iter()
    .find(|w| w.id().ok() == Some(id))
    .ok_or(DesktopError::StaleSnapshot)
}

pub(super) fn observe(
  scope: &TargetScope,
  target: Option<&super::targets::TargetBinding>,
  limits: ObservationLimits,
  control: &OperationControl,
) -> Result<DesktopObservation, DesktopError> {
  if let Some(target) = target {
    target.check_scope(scope)?;
  }
  let (bounds, image, windows, windows_truncated) = match scope {
    TargetScope::Display(id) => {
      let monitor = monitor(*id)?;
      let bounds = monitor_bounds(&monitor)?;
      // xcap macOS returns logical monitor dimensions but physical image pixels.
      // Validate a conservative scale bound before allowing upstream allocation.
      preflight_image(bounds, monitor.scale_factor().map_err(failure)?, limits)?;
      control.checkpoint(std::time::Instant::now())?;
      let image = monitor.capture_image().map_err(failure)?;
      let all = Window::all().map_err(failure)?;
      let mut windows = Vec::new();
      let mut truncated = false;
      for item in all {
        control.checkpoint(std::time::Instant::now())?;
        let data = window_data(&item)?;
        if intersects(bounds, data.bounds) {
          if windows.len() == limits.max_windows {
            truncated = true;
            break;
          }
          windows.push(data);
        }
      }
      (bounds, image, windows, truncated)
    }
    TargetScope::Window(id) => {
      let window = window(*id)?;
      if window.is_minimized().map_err(failure)? {
        return Err(DesktopError::SessionUnavailable);
      }
      let data = window_data(&window)?;
      if let Some(target) = target {
        target.check_identity(&TargetScope::Window(data.id), Some(data.process_id))?;
      }
      let scale = window
        .current_monitor()
        .map_err(failure)?
        .scale_factor()
        .map_err(failure)?;
      preflight_image(data.bounds, scale, limits)?;
      control.checkpoint(std::time::Instant::now())?;
      let image = window.capture_image().map_err(failure)?;
      (data.bounds, image, vec![data], false)
    }
  };
  control.checkpoint(std::time::Instant::now())?;
  limits.check_image(image.width(), image.height())?;
  Ok(DesktopObservation {
    bounds,
    screenshot: Screenshot {
      width: image.width(),
      height: image.height(),
      rgba: image.into_raw(),
    },
    windows,
    windows_truncated,
    accessibility: AccessibilityObservation {
      state: PermissionState::Unsupported,
      nodes: vec![],
      truncated: false,
    },
  })
}

fn preflight_image(
  bounds: Bounds,
  scale: f32,
  limits: ObservationLimits,
) -> Result<(), DesktopError> {
  if !scale.is_finite() || scale <= 0.0 || scale > 8.0 {
    return Err(DesktopError::CapacityExceeded);
  }
  #[cfg(target_os = "macos")]
  let factor = scale.max(1.0);
  #[cfg(not(target_os = "macos"))]
  let factor = 1.0_f32;
  let width = (f64::from(bounds.width) * f64::from(factor)).ceil();
  let height = (f64::from(bounds.height) * f64::from(factor)).ceil();
  if width > f64::from(u32::MAX) || height > f64::from(u32::MAX) {
    return Err(DesktopError::CapacityExceeded);
  }
  limits.check_image(width as u32, height as u32)
}

fn intersects(a: Bounds, b: Bounds) -> bool {
  i64::from(a.x) < i64::from(b.x) + i64::from(b.width)
    && i64::from(b.x) < i64::from(a.x) + i64::from(a.width)
    && i64::from(a.y) < i64::from(b.y) + i64::from(b.height)
    && i64::from(b.y) < i64::from(a.y) + i64::from(a.height)
}

pub(super) fn validate(snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
  let observed = &snapshot.observation;
  match snapshot.session.scope {
    TargetScope::Window(id) => {
      let current = window_data(&window(id)?)?;
      let expected = observed
        .windows
        .iter()
        .find(|window| window.id == id)
        .ok_or(DesktopError::StaleSnapshot)?;
      if !current.focused || !same_target(&current, expected) {
        return Err(DesktopError::StaleSnapshot);
      }
    }
    TargetScope::Display(id) => {
      if monitor_bounds(&monitor(id)?)? != observed.bounds {
        return Err(DesktopError::StaleSnapshot);
      }
      // Missing focus is not a safe target for keyboard/text/scroll either.
      let expected = observed
        .windows
        .iter()
        .find(|w| w.focused)
        .ok_or(DesktopError::StaleSnapshot)?;
      let current = Window::all()
        .map_err(failure)?
        .into_iter()
        .find(|w| w.is_focused().ok() == Some(true))
        .ok_or(DesktopError::StaleSnapshot)?;
      if !same_target(&window_data(&current)?, expected) {
        return Err(DesktopError::StaleSnapshot);
      }
    }
  }
  Ok(())
}

// Titles may legitimately change as a result of approved input. Full pre-input
// content validation checks them; post-input safety retains identity and geometry.
fn same_target(current: &WindowObservation, expected: &WindowObservation) -> bool {
  current.id == expected.id
    && current.process_id == expected.process_id
    && current.bounds == expected.bounds
    && current.focused == expected.focused
}
