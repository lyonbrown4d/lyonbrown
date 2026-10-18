//! Native setup elevation. Only a protected installed helper and public SID cross UAC.
use super::{
  WindowsSetupError,
  identity::{Identity, check, wide},
  protection::validate_setup_executable,
};
use anyhow::{Context, Result, ensure};
use std::{
  mem::size_of,
  os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
  path::Path,
};
use windows_sys::Win32::{
  Foundation::{ERROR_CANCELLED, GetLastError, WAIT_OBJECT_0},
  Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
  System::{
    Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize},
    Threading::{
      GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
    },
  },
  UI::{
    Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
    WindowsAndMessaging::SW_SHOWNORMAL,
  },
};

/// Canonical SID of the actual installing user, independent of account display names.
pub fn current_user_sid() -> Result<String> {
  Identity::current()?.sid_string()
}

/// Run before SCM queries, credential UI, configuration loading or other setup side effects.
pub fn validate_expected_user_sid(expected: &str) -> Result<()> {
  ensure!(
    expected == current_user_sid()?,
    WindowsSetupError::SetupUserMismatch
  );
  Ok(())
}

pub fn setup_is_elevated() -> Result<bool> {
  let mut raw = std::ptr::null_mut();
  // SAFETY: valid process pseudo-handle and writable token output.
  check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) })?;
  // SAFETY: successful OpenProcessToken returned an owned token handle.
  let token = unsafe { OwnedHandle::from_raw_handle(raw) };
  let mut elevation = TOKEN_ELEVATION::default();
  let mut bytes = 0;
  // SAFETY: live token and correctly sized/aligned elevation output.
  check(unsafe {
    GetTokenInformation(
      token.as_raw_handle(),
      TokenElevation,
      (&mut elevation as *mut TOKEN_ELEVATION).cast(),
      size_of::<TOKEN_ELEVATION>() as u32,
      &mut bytes,
    )
  })?;
  Ok(elevation.TokenIsElevated != 0)
}

struct ComApartment;
impl ComApartment {
  fn initialize() -> Result<Self> {
    // SAFETY: native setup owns this calling thread; no COM pointers cross threads.
    let status = unsafe {
      CoInitializeEx(
        std::ptr::null(),
        (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
      )
    };
    ensure!(
      status >= 0,
      "initialize Windows native setup COM apartment failed ({status:#x})"
    );
    Ok(Self)
  }
}
impl Drop for ComApartment {
  fn drop(&mut self) {
    // SAFETY: balanced with this thread's successful CoInitializeEx, including S_FALSE.
    unsafe {
      CoUninitialize();
    }
  }
}

/// Elevate the protected installed setup once and wait for the user's native setup.
/// The command line carries only the original public SID, never any credentials.
pub fn elevate_service_setup(current_exe: &Path, original_sid: &str) -> Result<u32> {
  validate_expected_user_sid(original_sid)?;
  let executable = validate_setup_executable(current_exe)?;
  ensure!(
    !setup_is_elevated()?,
    "native service setup is already elevated; refusing repeated elevation"
  );
  let file = wide(executable.as_os_str())?;
  let parent = executable
    .parent()
    .context("native setup executable has no parent directory")?;
  let directory = wide(parent.as_os_str())?;
  let verb = wide("runas".as_ref())?;
  // Canonical SID equality above guarantees that no caller-supplied command syntax is accepted.
  let parameters = wide(format!("--register-service --expected-user-sid {original_sid}").as_ref())?;
  let _apartment = ComApartment::initialize()?;
  let mut execution = SHELLEXECUTEINFOW {
    cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
    fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
    lpVerb: verb.as_ptr(),
    lpFile: file.as_ptr(),
    lpParameters: parameters.as_ptr(),
    lpDirectory: directory.as_ptr(),
    nShow: SW_SHOWNORMAL,
    ..Default::default()
  };
  // SAFETY: initialized structure with live terminated UTF-16 inputs. Public runas
  // delegates consent and administrator authentication exclusively to native UAC.
  if unsafe { ShellExecuteExW(&mut execution) } == 0 {
    // SAFETY: capture last-error before cleanup or any other Windows call.
    let error = unsafe { GetLastError() };
    if error == ERROR_CANCELLED {
      return Err(WindowsSetupError::ElevationCancelled.into());
    }
    return Err(std::io::Error::from_raw_os_error(error as i32))
      .context("request native Windows service setup elevation");
  }
  ensure!(
    !execution.hProcess.is_null(),
    "Windows elevation returned no setup process; setup outcome is unknown"
  );
  // SAFETY: SEE_MASK_NOCLOSEPROCESS transfers this valid process handle to us.
  let process = unsafe { OwnedHandle::from_raw_handle(execution.hProcess) };
  // SAFETY: live process handle with SYNCHRONIZE access. User credential entry has
  // no artificial deadline; waiting does not cancel or replay an unknown setup.
  let wait = unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) };
  if wait != WAIT_OBJECT_0 {
    // Capture OS failure before dropping the owned process handle.
    return Err(std::io::Error::last_os_error())
      .context("wait for elevated Windows setup; outcome is unknown");
  }
  let mut exit = 0;
  // SAFETY: the child has terminated; live handle and writable exit-code output.
  check(unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut exit) })?;
  Ok(exit)
}
