//! CEF execution environment composed into native clients, never another agent loop.
#[cfg(feature = "native-cef")]
mod composition;
#[cfg(feature = "native-cef")]
mod native;

mod attachment;
mod cache_root;
pub use agent_browser::BrowserHost as BrowserRuntime;
pub use attachment::{RuntimeAttachment, attach, attach_executable, attach_host};
pub use cache_root::CacheRootLease;
use std::{ffi::OsString, process::ExitCode};

pub async fn launch(profiles: &std::path::Path) -> agent_browser::BrowserResult<BrowserRuntime> {
  let executable = std::env::current_exe()
    .map_err(|error| agent_browser::BrowserError::Unavailable(error.to_string()))?;
  BrowserRuntime::spawn(&executable, profiles).await
}

/// Dispatch before the client's TUI/GPUI event loop and before command-line parsing.
/// CEF helper processes reuse the installed client executable.
pub fn dispatch_entry() -> anyhow::Result<Option<ExitCode>> {
  dispatch(false)
}

/// Dev-only synthetic profile acceptance. Production client entries never call
/// this function: Chromium's test keychain does not protect real browser data.
#[cfg(debug_assertions)]
pub fn dispatch_acceptance_entry() -> anyhow::Result<Option<ExitCode>> {
  dispatch(true)
}

fn dispatch(fixture_keychain: bool) -> anyhow::Result<Option<ExitCode>> {
  let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
  let requested = arguments.iter().any(|argument| {
    argument == "--browser-runtime" || argument.to_string_lossy().starts_with("--type=")
  });
  if !requested {
    return Ok(None);
  }
  #[cfg(feature = "native-cef")]
  {
    native::run(arguments, std::ptr::null_mut(), None, fixture_keychain)
      .map(|status| Some(ExitCode::from(status.clamp(0, 255) as u8)))
  }
  #[cfg(not(feature = "native-cef"))]
  {
    let _ = fixture_keychain;
    anyhow::bail!(
      "CEF BrowserRuntime is unavailable in this client build; build with native-cef and package the matching CEF SDK"
    )
  }
}

pub const fn runtime_available() -> bool {
  cfg!(feature = "native-cef")
}

/// Entry used only by the maintained CEF Windows sandbox bootstrap.
///
/// # Safety
/// `instance` and `sandbox` must be the live values passed by cef-rs bootstrap.exe
/// to RunWinMain; this runs on that process's initial thread before any UI loop.
#[cfg(all(target_os = "windows", feature = "native-cef"))]
pub unsafe fn windows_dispatch(
  instance: *mut std::ffi::c_void,
  sandbox: *mut u8,
) -> anyhow::Result<Option<i32>> {
  let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
  if !arguments.iter().any(|argument| {
    argument == "--browser-runtime" || argument.to_string_lossy().starts_with("--type=")
  }) {
    return Ok(None);
  }
  native::run(arguments, sandbox, Some(instance), false).map(Some)
}

/// Export RunWinMain from the native client's DLL; the executable remains the product.
#[macro_export]
macro_rules! windows_bootstrap_entry {
  ($run:path) => {
    #[cfg(all(target_os = "windows", feature = "native-cef"))]
    #[unsafe(no_mangle)]
    /// # Safety
    /// Called only by the matching maintained CEF sandbox bootstrap on its initial thread.
    pub unsafe extern "C" fn RunWinMain(
      instance: *mut ::std::ffi::c_void,
      _command_line: *const u8,
      _command_show: i32,
      sandbox: *mut u8,
    ) -> i32 {
      // SAFETY: the matching CEF bootstrap owns the pointers for this call's full duration.
      match unsafe { $crate::windows_dispatch(instance, sandbox) } {
        Ok(Some(status)) => status,
        Ok(None) => match $run() {
          Ok(status) if status == ::std::process::ExitCode::SUCCESS => 0,
          _ => 1,
        },
        Err(error) => {
          eprintln!("BrowserRuntime bootstrap failed: {error}");
          1
        }
      }
    }
  };
}
