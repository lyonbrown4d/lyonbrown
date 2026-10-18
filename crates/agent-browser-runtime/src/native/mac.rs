//! macOS-only CEF loading and NSApplication protocol adaptation.
use anyhow::{Context, ensure};
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
  ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
  runtime::Bool,
};
use objc2_app_kit::{NSApplication, NSEvent};
use std::{cell::Cell, ffi::CString, os::unix::ffi::OsStrExt, path::PathBuf};

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
  static kCFRunLoopDefaultMode: *const std::ffi::c_void;
  fn CFRunLoopRunInMode(
    mode: *const std::ffi::c_void,
    seconds: f64,
    return_after_source: u8,
  ) -> i32;
}

pub fn pump_observers() {
  // SAFETY: the CEF browser loop runs only on the initial Cocoa thread. The
  // framework owns this mode constant. A bounded iteration lets native run-loop
  // observers deliver compositor and navigation work, as in CEF's macOS pump.
  unsafe {
    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.001, 1);
    let application: Retained<RuntimeApplication> =
      msg_send![RuntimeApplication::class(), sharedApplication];
    let date: Retained<objc2::runtime::AnyObject> = msg_send![objc2::class!(NSDate), distantPast];
    let mode: Retained<objc2::runtime::AnyObject> =
      msg_send![objc2::class!(NSString), stringWithUTF8String: c"kCFRunLoopDefaultMode".as_ptr()];
    // Like NSApplication::run, dispatch pending native events as well as run-loop
    // observers. Never wait for an event or starve the bounded IPC queue.
    for _ in 0..32 {
      let event: Option<Retained<NSEvent>> = msg_send![&*application, nextEventMatchingMask: usize::MAX, untilDate: &*date, inMode: &*mode, dequeue: true];
      let Some(event) = event else { break };
      let _: () = msg_send![&*application, sendEvent: &*event];
    }
  }
}

pub fn finish_launching() {
  // SAFETY: the native browser created this NSApplication on its initial thread.
  // NSApplication::run normally completes launch; our bounded IPC loop replaces
  // that method and must perform the same documented launch step explicitly.
  unsafe {
    let application: Retained<RuntimeApplication> =
      msg_send![RuntimeApplication::class(), sharedApplication];
    let _: () = msg_send![&*application, finishLaunching];
  }
}

#[derive(Default)]
pub struct ApplicationState {
  sending: Cell<Bool>,
}

define_class! {
  #[unsafe(super(NSApplication))]
  #[thread_kind = MainThreadOnly]
  #[ivars = ApplicationState]
  struct RuntimeApplication;

  impl RuntimeApplication {
    #[unsafe(method(sendEvent:))]
    unsafe fn send_event(&self, event: &NSEvent) {
      let previous = self.ivars().sending.replace(Bool::YES);
      // SAFETY: Cocoa invokes this override on its main-thread NSApplication with a live NSEvent.
      let _: () = unsafe { msg_send![super(self), sendEvent: event] };
      self.ivars().sending.set(previous);
    }
  }
  // SAFETY: these protocol methods expose only main-thread Cell state owned by this subclass.
  unsafe impl CrAppProtocol for RuntimeApplication {
    #[unsafe(method(isHandlingSendEvent))]
    unsafe fn handling(&self) -> Bool { self.ivars().sending.get() }
  }
  unsafe impl CrAppControlProtocol for RuntimeApplication {
    #[unsafe(method(setHandlingSendEvent:))]
    unsafe fn set_handling(&self, value: Bool) { self.ivars().sending.set(value); }
  }
  unsafe impl CefAppProtocol for RuntimeApplication {}
}

pub struct LoadedFramework {
  pub directory: PathBuf,
}

impl Drop for LoadedFramework {
  fn drop(&mut self) {
    let _ = cef::unload_library();
  }
}

pub fn initialize(create_application: bool) -> anyhow::Result<LoadedFramework> {
  let _main = MainThreadMarker::new()
    .context("CEF BrowserRuntime must start on the main thread before GPUI/TUI")?;
  let executable = std::env::current_exe()?;
  let parent = executable
    .parent()
    .context("client executable has no directory")?;
  let configured = std::env::var_os("CEF_PATH").map(PathBuf::from);
  let framework = "Chromium Embedded Framework.framework/Chromium Embedded Framework";
  let candidates = [
    Some(parent.join("../Frameworks").join(framework)),
    Some(parent.join("../../..").join(framework)),
    configured.map(|path| path.join(framework)),
    cef::sys::get_cef_dir().map(|path| path.join(framework)),
  ];
  let library = candidates.into_iter().flatten().find(|path| path.is_file())
    .context("CEF framework missing; package the matching framework in Contents/Frameworks or set CEF_PATH")?;
  let directory = library
    .parent()
    .context("CEF framework has no directory")?
    .to_path_buf();
  let library =
    CString::new(library.as_os_str().as_bytes()).context("CEF framework path contains NUL")?;
  // SAFETY: CString is NUL-terminated, remains alive for this call, and targets a validated local framework binary.
  let loaded = unsafe { cef::load_library(Some(&*library.as_ptr().cast())) };
  ensure!(loaded == 1, "CEF framework could not be loaded");
  if create_application {
    // SAFETY: this is the browser process's first NSApplication access, on its initial thread.
    let _application: Retained<RuntimeApplication> =
      unsafe { msg_send![RuntimeApplication::class(), sharedApplication] };
  }
  Ok(LoadedFramework { directory })
}

pub fn helper_executable(executable: &std::path::Path) -> anyhow::Result<PathBuf> {
  let name = executable
    .file_name()
    .and_then(|name| name.to_str())
    .context("native client executable has no UTF-8 filename")?;
  let bundle = executable
    .parent()
    .and_then(|path| path.parent())
    .and_then(|path| path.parent())
    .context("native client app bundle is missing")?;
  ensure!(
    bundle
      .extension()
      .is_some_and(|extension| extension == "app"),
    "macOS CEF requires the native client app bundle and embedded helper bundles; run the packaged client executable"
  );
  let helper = format!("{name} Helper");
  let path = bundle
    .join("Contents/Frameworks")
    .join(format!("{helper}.app/Contents/MacOS/{helper}"));
  ensure!(
    path.is_file(),
    "native client CEF helper bundle is missing: {}",
    path.display()
  );
  Ok(path)
}

/// The maintained CEF sandbox must initialize before loading Chromium in helper processes.
pub fn sandbox(args: &cef::MainArgs) -> anyhow::Result<cef::sandbox::Sandbox> {
  let executable = std::env::current_exe()?;
  let library = executable
    .parent()
    .context("CEF helper has no directory")?
    .join("../../../Chromium Embedded Framework.framework/Libraries/libcef_sandbox.dylib");
  ensure!(
    library.is_file(),
    "CEF sandbox library is missing from the product helper bundle"
  );
  let mut sandbox = cef::sandbox::Sandbox::new();
  sandbox.initialize(args);
  Ok(sandbox)
}
