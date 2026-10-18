//! Private SCM entrypoint. Ordinary console execution never impersonates a service.
use anyhow::{Context, Result, ensure};
use std::{ffi::OsString, time::Duration};
use tokio_util::sync::CancellationToken;
use windows_service::{
  define_windows_service,
  service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
  },
  service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
  service_dispatcher,
};

pub fn requested_name() -> Result<Option<String>> {
  name_from_args(std::env::args_os().skip(1))
}

fn name_from_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<String>> {
  let mut args = args.into_iter();
  let mut service = None;
  while let Some(arg) = args.next() {
    let value = if arg == "--windows-service" {
      Some(
        args
          .next()
          .context("private SCM runtime option requires service identity")?,
      )
    } else {
      arg
        .to_str()
        .and_then(|arg| arg.strip_prefix("--windows-service="))
        .map(OsString::from)
    };
    let Some(name) = value else {
      continue;
    };
    ensure!(service.is_none(), "duplicate private SCM runtime option");
    let name = name
      .into_string()
      .map_err(|_| anyhow::anyhow!("invalid SCM service identity"))?;
    ensure!(
      name.len() == 81
        && name.starts_with("Lyonbrown.Agentd.")
        && name[17..].chars().all(|c| c.is_ascii_hexdigit()),
      "invalid SCM service identity"
    );
    service = Some(name);
  }
  Ok(service)
}

pub fn dispatch(name: &str) -> Result<()> {
  service_dispatcher::start(name, ffi_service_main)
    .context("connect agentd to Windows SCM; this private mode must be launched by SCM")
}
define_windows_service!(ffi_service_main, service_main);

fn service_main(arguments: Vec<OsString>) {
  // SCM passes its service identity as the first argument to the entrypoint.
  let Some(name) = arguments.first() else {
    return;
  };
  let stop = CancellationToken::new();
  let shutdown = stop.clone();
  let handle = match service_control_handler::register(name, move |control| match control {
    ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
    ServiceControl::Stop | ServiceControl::Shutdown => {
      shutdown.cancel();
      ServiceControlHandlerResult::NoError
    }
    _ => ServiceControlHandlerResult::NotImplemented,
  }) {
    Ok(handle) => handle,
    Err(_) => return,
  };
  let result = run(&handle, stop);
  // Log files are initialized by run; do not serialize config or credentials here.
  if let Err(error) = &result {
    tracing::error!(error = %error, "Windows service runtime failed");
  }
  let _ = handle.set_service_status(status(
    ServiceState::Stopped,
    if result.is_ok() { 0 } else { 1 },
    0,
  ));
}

#[tokio::main]
async fn run(handle: &ServiceStatusHandle, stop: CancellationToken) -> Result<()> {
  handle.set_service_status(status(ServiceState::StartPending, 0, 1))?;
  // Keep SCM startup checkpoints advancing while persisted jobs are recovered.
  let ready = CancellationToken::new();
  let ready_signal = ready.clone();
  let config = agentd::config::Config::load_service()?;
  ensure!(
    config.maintenance.is_none() && !config.compatibility_json,
    "SCM runtime does not accept console maintenance modes"
  );
  let shutdown_handle = *handle;
  let monitor_stop = stop.clone();
  let shutdown = async {
    stop.cancelled().await;
    let _ = shutdown_handle.set_service_status(status(ServiceState::StopPending, 0, 1));
  };
  let serve = super::run(config, shutdown, || {
    handle.set_service_status(status(ServiceState::Running, 0, 0))?;
    ready_signal.cancel();
    Ok(())
  });
  let pending = async {
    let mut checkpoint = 1_u32;
    loop {
      tokio::time::sleep(Duration::from_secs(5)).await;
      let state = if monitor_stop.is_cancelled() {
        ServiceState::StopPending
      } else if !ready.is_cancelled() {
        ServiceState::StartPending
      } else {
        continue;
      };
      checkpoint = checkpoint.saturating_add(1);
      if let Err(error) = handle.set_service_status(status(state, 0, checkpoint)) {
        tracing::warn!(error = %error, "SCM progress status update failed");
      }
    }
  };
  tokio::pin!(serve, pending);
  tokio::select! {
    result = &mut serve => result,
    _ = &mut pending => Err(anyhow::anyhow!("SCM progress monitor stopped unexpectedly")),
  }
}

fn status(state: ServiceState, code: u32, checkpoint: u32) -> ServiceStatus {
  ServiceStatus {
    service_type: ServiceType::OWN_PROCESS,
    current_state: state,
    controls_accepted: if state == ServiceState::Running {
      ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
    } else {
      ServiceControlAccept::empty()
    },
    exit_code: ServiceExitCode::Win32(code),
    checkpoint,
    wait_hint: if checkpoint == 0 {
      Duration::ZERO
    } else {
      Duration::from_secs(15)
    },
    process_id: None,
  }
}
