mod context;
mod handlers;
#[cfg(target_os = "macos")]
mod mac;
mod privileges;
mod state;
mod tools;

use agent_browser::wire::{self, Request, Response};
use anyhow::{Context, ensure};
use cef::{args::Args, *};
use std::{ffi::OsString, io, path::PathBuf, sync::mpsc, time::Duration};

pub fn run(
  arguments: Vec<OsString>,
  sandbox: *mut u8,
  instance: Option<*mut std::ffi::c_void>,
  fixture_keychain: bool,
) -> anyhow::Result<i32> {
  #[cfg(target_os = "windows")]
  let args = Args::from(MainArgs {
    instance: instance.context("Windows CEF requires the sandbox bootstrap DLL entrypoint")?,
  });
  #[cfg(not(target_os = "windows"))]
  let args = {
    let _ = instance;
    Args::new()
  };
  #[cfg(target_os = "macos")]
  let helper = arguments
    .iter()
    .any(|argument| argument.to_string_lossy().starts_with("--type="));
  #[cfg(target_os = "macos")]
  let _sandbox = if helper {
    Some(mac::sandbox(args.as_main_args())?)
  } else {
    None
  };
  #[cfg(target_os = "macos")]
  let _library = mac::initialize(!helper)?;
  ensure!(
    !api_hash(sys::CEF_API_VERSION_LAST, 0).is_null(),
    "CEF runtime does not support the API required by this client; package the matching SDK"
  );
  let context_ready = std::rc::Rc::new(std::cell::Cell::new(false));
  let mut app = handlers::RuntimeApp::new(fixture_keychain, context_ready.clone());
  let result = execute_process(Some(args.as_main_args()), Some(&mut app), sandbox);
  if result >= 0 {
    return Ok(result);
  }
  let profiles = arguments
    .windows(2)
    .find(|pair| pair[0] == "--profiles")
    .map(|pair| PathBuf::from(&pair[1]))
    .context("BrowserRuntime requires --profiles PATH")?;
  let module = crate::composition::RuntimeModule::new(fluxdi::Shared::new(
    crate::composition::RuntimeConfiguration { profiles },
  ));
  let profiles = module.profiles()?.0.clone();
  // Fail before CEF initialization; Chromium otherwise handles a root collision
  // as an application relaunch instead of returning our runtime error contract.
  let _cache_root = crate::CacheRootLease::acquire(&profiles)?;
  let logs = profiles.join("runtime-logs");
  let _logs = agent_telemetry::init_with_options(
    &agent_telemetry::LogOptions {
      directory: logs.clone(),
      retained_files: 14,
      stderr: false,
    },
    "browser-runtime",
    "info",
  )?;
  tracing::info!(
    event = "browser_runtime_started",
    protocol_version = agent_browser::RUNTIME_PROTOCOL_VERSION,
    fixture_keychain,
    "Native CEF execution environment starting"
  );
  let executable = std::env::current_exe()?;
  #[cfg(target_os = "macos")]
  let executable = mac::helper_executable(&executable)?;
  let settings = Settings {
    windowless_rendering_enabled: 1,
    // A bounded periodic loop calls CefDoMessageLoopWork. External scheduling is
    // disabled because this process does not implement OnScheduleMessagePumpWork.
    external_message_pump: 0,
    root_cache_path: profiles.to_string_lossy().as_ref().into(),
    browser_subprocess_path: executable.to_string_lossy().as_ref().into(),
    no_sandbox: 0,
    log_file: logs.join("chromium.log").to_string_lossy().as_ref().into(),
    log_severity: LogSeverity::ERROR,
    #[cfg(target_os = "macos")]
    framework_dir_path: _library.directory.to_string_lossy().as_ref().into(),
    ..Default::default()
  };
  ensure!(
    initialize(
      Some(args.as_main_args()),
      Some(&settings),
      Some(&mut app),
      sandbox
    ) == 1,
    "CEF initialization failed; verify matching SDK, resources and sandbox packaging"
  );
  #[cfg(target_os = "macos")]
  mac::finish_launching();
  let deadline = std::time::Instant::now() + Duration::from_secs(10);
  while !context_ready.get() {
    pump();
    if std::time::Instant::now() >= deadline {
      shutdown();
      anyhow::bail!("CEF browser context initialization timed out");
    }
    std::thread::sleep(Duration::from_millis(4));
  }
  let result = serve(profiles);
  shutdown();
  result?;
  Ok(0)
}

fn serve(profiles: PathBuf) -> anyhow::Result<()> {
  let (sender, receiver) = mpsc::sync_channel::<Request>(8);
  std::thread::Builder::new()
    .name("browser-runtime-ipc".into())
    .spawn(move || {
      let mut input = io::stdin().lock();
      while let Ok(request) = wire::read_packet(&mut input) {
        if sender.send(request).is_err() {
          break;
        }
      }
    })?;
  let mut output = io::stdout().lock();
  let mut engine = state::Engine::new(profiles);
  let result = loop {
    pump();
    match receiver.try_recv() {
      Ok(request) => {
        let stop = matches!(request.command, agent_browser::Command::Shutdown);
        let response = Response {
          id: request.id,
          result: engine.execute(request.command),
        };
        if let Err(error) = wire::write_packet(&mut output, &response) {
          break Err(error.into());
        }
        if stop {
          break Ok(());
        }
      }
      Err(mpsc::TryRecvError::Disconnected) => break Ok(()),
      Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(8)),
    }
  };
  if engine.close_all().is_err() {
    // CEF requires every OnBeforeClose before shutdown/unloading. Process termination
    // releases OS resources without invoking CEF through partially closed objects.
    eprintln!("CEF close timed out; terminating runtime without unloading active browser objects");
    std::process::exit(1);
  }
  result
}

fn pump() {
  #[cfg(target_os = "macos")]
  mac::pump_observers();
  do_message_loop_work();
}
