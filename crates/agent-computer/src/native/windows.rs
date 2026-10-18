use std::mem::{size_of, size_of_val};

use ::windows::{
  Win32::{
    Foundation::HANDLE,
    System::{RemoteDesktop::*, StationsAndDesktops::*, Threading::GetCurrentProcessId},
  },
  core::PWSTR,
};

use crate::*;

struct DesktopHandle(HDESK);

impl Drop for DesktopHandle {
  fn drop(&mut self) {
    // SAFETY: owned non-null OpenInputDesktop handle, closed exactly once.
    let _ = unsafe { CloseDesktop(self.0) };
  }
}

pub(super) fn session() -> Result<String, DesktopError> {
  let mut session_id = 0;
  // SAFETY: process ID is current process; session output points to a live u32.
  unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }
    .map_err(|_| DesktopError::SessionUnavailable)?;
  if session_id == 0 {
    return Err(DesktopError::SessionUnavailable);
  }
  let mut buffer = PWSTR::null();
  let mut bytes = 0;
  // SAFETY: WTS returns an allocated buffer via the out pointer, freed below.
  unsafe {
    WTSQuerySessionInformationW(None, session_id, WTSConnectState, &mut buffer, &mut bytes)
  }
  .map_err(|_| DesktopError::SessionUnavailable)?;
  let active = if !buffer.is_null() && bytes as usize >= size_of::<WTS_CONNECTSTATE_CLASS>() {
    // SAFETY: size checked; WTS documents WTSConnectState as this enum.
    unsafe { *buffer.0.cast::<WTS_CONNECTSTATE_CLASS>() == WTSActive }
  } else {
    false
  };
  if !buffer.is_null() {
    // SAFETY: buffer is the non-null allocation returned by WTS.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
  }
  if !active {
    return Err(DesktopError::SessionUnavailable);
  }
  // Access to the input desktop fails at the locked/secure desktop boundary.
  // Never switch desktops or raise privileges in an attempt to work around it.
  let desktop = DesktopHandle(
    unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) }
      .map_err(|_| DesktopError::Locked)?,
  );
  let mut name = [0_u16; 256];
  let mut needed = 0;
  // SAFETY: handle is owned/live, writable buffer byte length matches allocation.
  unsafe {
    GetUserObjectInformationW(
      HANDLE(desktop.0.0),
      UOI_NAME,
      Some(name.as_mut_ptr().cast()),
      size_of_val(&name) as u32,
      Some(&mut needed),
    )
  }
  .map_err(|_| DesktopError::SessionUnavailable)?;
  let end = name
    .iter()
    .position(|unit| *unit == 0)
    .ok_or(DesktopError::SessionUnavailable)?;
  if !String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case("Default") {
    return Err(DesktopError::Locked);
  }
  Ok(format!("windows:{session_id}"))
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
