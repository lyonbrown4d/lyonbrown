use core_foundation::{
  base::{CFType, TCFType},
  boolean::CFBoolean,
  dictionary::{CFDictionary, CFDictionaryRef},
  string::CFString,
};
use core_graphics::access::ScreenCaptureAccess;

use crate::*;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
  fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
}

pub(super) fn session() -> Result<String, DesktopError> {
  let before = session_snapshot()?;
  let locked = console_locked();
  let identity = before.validate(locked)?;
  let after = session_snapshot()?;
  after.validate(console_locked())?;
  if before != after {
    return Err(DesktopError::SessionUnavailable);
  }
  Ok(identity)
}

#[derive(Debug, PartialEq)]
struct SessionSnapshot {
  on_console: Option<bool>,
  login_done: Option<bool>,
  lock_hint: Option<bool>,
  identity: Option<uuid::Uuid>,
}

impl SessionSnapshot {
  fn validate(&self, console_locked: Option<bool>) -> Result<String, DesktopError> {
    if self.lock_hint == Some(true) || console_locked == Some(true) {
      return Err(DesktopError::Locked);
    }
    if self.on_console != Some(true) || self.login_done != Some(true) {
      return Err(DesktopError::SessionUnavailable);
    }
    if console_locked != Some(false) {
      return Err(DesktopError::SessionStateUnknown);
    }
    let identity = self
      .identity
      .filter(|id| !id.is_nil())
      .ok_or(DesktopError::SessionStateUnknown)?;
    Ok(format!("macos:{identity}"))
  }
}

fn session_snapshot() -> Result<SessionSnapshot, DesktopError> {
  // SAFETY: Apple's Copy API returns NULL or a +1 CFDictionary. The owned
  // dictionary releases that reference; all values retain their proper CF type.
  let raw = unsafe { CGSessionCopyCurrentDictionary() };
  if raw.is_null() {
    return Err(DesktopError::SessionUnavailable);
  }
  let dictionary: CFDictionary<CFString, CFType> =
    unsafe { CFDictionary::wrap_under_create_rule(raw) };
  let boolean = |key: &str| -> Option<bool> {
    dictionary
      .find(CFString::new(key))
      .and_then(|value| value.downcast::<CFBoolean>())
      .map(bool::from)
  };
  let identity = dictionary
    .find(CFString::new("CGSSessionUniqueSessionUUID"))
    .and_then(|value| value.downcast::<CFString>())
    .and_then(|value| uuid::Uuid::parse_str(&value.to_string()).ok());
  Ok(SessionSnapshot {
    on_console: boolean("kCGSSessionOnConsoleKey"),
    login_done: boolean("kCGSessionLoginDoneKey"),
    lock_hint: boolean("CGSSessionScreenIsLocked"),
    identity,
  })
}

fn console_locked() -> Option<bool> {
  use objc2_core_foundation::{CFBoolean, CFString};
  use objc2_io_kit::{
    IOObjectRelease, IORegistryEntryCreateCFProperty, IORegistryGetRootEntry, kIOMainPortDefault,
  };
  // SAFETY: this framework-owned constant identifies the caller's default IOKit port.
  let root = IORegistryGetRootEntry(unsafe { kIOMainPortDefault });
  if root == 0 {
    return None;
  }
  // IOConsoleLocked is an OS implementation field, not a documented lock-state
  // contract. The chosen compatibility policy requires an explicit false value;
  // absent/malformed data never authorizes control. No shell command is spawned.
  let key = CFString::from_str("IOConsoleLocked");
  // SAFETY: root is live, key is a retained CFString, NULL selects Apple's default
  // allocator and zero selects the documented default options. Release root once;
  // the returned property owns its separate reference through CFRetained.
  let value = unsafe { IORegistryEntryCreateCFProperty(root, Some(&key), None, 0) };
  let _ = IOObjectRelease(root);
  value
    .as_deref()
    .and_then(|value| value.downcast_ref::<CFBoolean>())
    .map(CFBoolean::as_bool)
}

pub(super) fn capture_permission() -> PermissionState {
  if ScreenCaptureAccess.preflight() {
    PermissionState::Granted
  } else {
    PermissionState::Denied
  }
}

pub(super) fn input_permission() -> PermissionState {
  // SAFETY: no pointers; the non-prompting trust query only reads OS state.
  if unsafe { accessibility_sys::AXIsProcessTrusted() } {
    PermissionState::Granted
  } else {
    PermissionState::Denied
  }
}

pub(super) fn accessibility_permission() -> PermissionState {
  input_permission()
}

pub(super) fn accessibility(
  windows: &[WindowObservation],
  limits: ObservationLimits,
  control: &OperationControl,
) -> Result<AccessibilityObservation, DesktopError> {
  super::macos_accessibility::observe(windows, limits, control)
}

#[cfg(test)]
#[path = "../../tests/support/macos_session.rs"]
mod tests;
