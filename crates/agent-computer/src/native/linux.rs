use zbus::{blocking::Proxy, zvariant::OwnedObjectPath};

use crate::*;

fn unavailable(_: impl std::fmt::Display) -> DesktopError {
  DesktopError::SessionUnavailable
}

pub(super) fn session() -> Result<String, DesktopError> {
  // xcap chooses its portal path from these environment variables. Reject mixed
  // environments instead of accidentally invoking an interactive portal prompt.
  if std::env::var("XDG_SESSION_TYPE").ok().as_deref() != Some("x11")
    || std::env::var_os("WAYLAND_DISPLAY").is_some()
  {
    return Err(DesktopError::Unsupported(
      "only an explicit X11 helper environment is supported".into(),
    ));
  }
  let connection = zbus::blocking::connection::Builder::system()
    .map_err(unavailable)?
    .method_timeout(std::time::Duration::from_millis(250))
    .build()
    .map_err(unavailable)?;
  let manager = Proxy::new(
    &connection,
    "org.freedesktop.login1",
    "/org/freedesktop/login1",
    "org.freedesktop.login1.Manager",
  )
  .map_err(unavailable)?;
  // PID is supplied by the kernel, not a model/environment-selected session ID.
  let path: OwnedObjectPath = manager
    .call("GetSessionByPID", &(std::process::id(),))
    .map_err(unavailable)?;
  let session = Proxy::new(
    &connection,
    "org.freedesktop.login1",
    path,
    "org.freedesktop.login1.Session",
  )
  .map_err(unavailable)?;
  let kind: String = session.get_property("Type").map_err(unavailable)?;
  if kind != "x11" {
    return Err(DesktopError::Unsupported(
      "native input requires X11; Wayland portal grants are not configured".into(),
    ));
  }
  let active: bool = session.get_property("Active").map_err(unavailable)?;
  let locked: bool = session.get_property("LockedHint").map_err(unavailable)?;
  let class: String = session.get_property("Class").map_err(unavailable)?;
  let state: String = session.get_property("State").map_err(unavailable)?;
  if locked {
    return Err(DesktopError::Locked);
  }
  if !active || class != "user" || state != "active" {
    return Err(DesktopError::SessionUnavailable);
  }
  let display: String = session.get_property("Display").map_err(unavailable)?;
  if display.is_empty() || std::env::var("DISPLAY").ok().as_deref() != Some(&display) {
    return Err(DesktopError::SessionUnavailable);
  }
  let id: String = session.get_property("Id").map_err(unavailable)?;
  Ok(format!("linux:{id}:{display}"))
}

pub(super) fn capture_permission() -> PermissionState {
  PermissionState::Granted
}
pub(super) fn input_permission() -> PermissionState {
  PermissionState::Granted
}
pub(super) fn accessibility_permission() -> PermissionState {
  PermissionState::Unsupported
}
pub(super) fn accessibility(
  _: &[WindowObservation],
  _: ObservationLimits,
  _: &OperationControl,
) -> Result<AccessibilityObservation, DesktopError> {
  Ok(AccessibilityObservation {
    state: PermissionState::Unsupported,
    nodes: vec![],
    truncated: false,
  })
}
