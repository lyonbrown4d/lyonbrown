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
  // This de facto field can veto execution, but is not a documented unlocked
  // state guarantee and must never authorize control.
  if boolean("CGSSessionScreenIsLocked") == Some(true) {
    return Err(DesktopError::Locked);
  }
  if boolean("kCGSSessionOnConsoleKey") != Some(true)
    || boolean("kCGSessionLoginDoneKey") != Some(true)
  {
    return Err(DesktopError::SessionUnavailable);
  }
  // Apple's public dictionary proves console/login state, not unlocked state.
  // This adapter therefore cannot advertise desktop control from this probe.
  Err(DesktopError::SessionStateUnknown)
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
