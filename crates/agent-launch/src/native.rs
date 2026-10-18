#[cfg(not(windows))]
use std::{
  fs,
  io::ErrorKind,
  path::PathBuf,
  time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
#[cfg(not(windows))]
use directories::BaseDirs;
#[cfg(not(windows))]
use service_manager::{
  RestartPolicy, ServiceInstallCtx, ServiceLevel, ServiceManager, ServiceManagerKind,
  ServiceStartCtx, ServiceStatus, ServiceStatusCtx, ServiceStopCtx, ServiceUninstallCtx,
};

#[cfg(not(windows))]
use crate::model::{NativeService, ServiceState};
use crate::model::{Registration, SERVICE_ID};
#[cfg(not(windows))]
use service_manager::ServiceLabel;

pub struct NativeManager;

#[cfg(windows)]
#[path = "windows/mod.rs"]
pub mod windows;

#[cfg(not(windows))]
fn manager() -> Result<Box<dyn ServiceManager>> {
  let kind = if cfg!(target_os = "macos") {
    ServiceManagerKind::Launchd
  } else if cfg!(target_os = "linux") {
    ServiceManagerKind::Systemd
  } else {
    bail!(
      "per-user native agentd service is unavailable on this platform; Windows SCM requires a service dispatcher and per-user identity provisioning"
    )
  };
  let mut manager = <dyn ServiceManager>::target(kind);
  manager.set_level(ServiceLevel::User)?;
  ensure!(
    manager.available()?,
    "native user service manager is unavailable"
  );
  Ok(manager)
}

#[cfg(not(windows))]
fn label() -> Result<ServiceLabel> {
  SERVICE_ID
    .parse()
    .context("invalid agentd service identity")
}

#[cfg(not(windows))]
fn definition_path() -> Result<PathBuf> {
  let dirs = BaseDirs::new().context("user home/config directories are unavailable")?;
  if cfg!(target_os = "macos") {
    Ok(
      dirs
        .home_dir()
        .join("Library/LaunchAgents")
        .join(format!("{SERVICE_ID}.plist")),
    )
  } else if cfg!(target_os = "linux") {
    Ok(
      dirs
        .config_dir()
        .join("systemd/user")
        .join(format!("{}.service", label()?.to_script_name())),
    )
  } else {
    bail!("native user service definition is unavailable on this platform")
  }
}

pub fn arguments(registration: &Registration) -> Vec<std::ffi::OsString> {
  vec![
    "--listen".into(),
    "127.0.0.1:7447".into(),
    "--data-dir".into(),
    registration.data_dir.as_os_str().to_owned(),
  ]
}

/// Stage a native definition without registering or starting anything.
pub fn definition(registration: &Registration) -> Result<String> {
  ensure!(
    registration.executable.is_absolute(),
    "native service executable must be absolute"
  );
  ensure!(
    registration.data_dir.is_absolute(),
    "native service data directory must be absolute"
  );
  if cfg!(target_os = "macos") {
    launchd_definition(registration)
  } else if cfg!(target_os = "linux") {
    systemd_definition(registration)
  } else {
    bail!("native per-user service definition is unsupported on this platform")
  }
}

pub fn launchd_definition(registration: &Registration) -> Result<String> {
  let mut dictionary = plist::Dictionary::new();
  dictionary.insert("Label".into(), SERVICE_ID.into());
  let mut args = vec![registration.executable.as_os_str().to_owned()];
  args.extend(arguments(registration));
  dictionary.insert(
    "ProgramArguments".into(),
    plist::Value::Array(
      args
        .iter()
        .map(|arg| {
          arg
            .to_str()
            .context("service paths must be UTF-8")
            .map(|arg| arg.into())
        })
        .collect::<Result<Vec<_>>>()?,
    ),
  );
  dictionary.insert(
    "WorkingDirectory".into(),
    registration
      .data_dir
      .to_str()
      .context("data directory must be UTF-8")?
      .into(),
  );
  dictionary.insert("RunAtLoad".into(), true.into());
  let mut keep_alive = plist::Dictionary::new();
  keep_alive.insert("SuccessfulExit".into(), false.into());
  dictionary.insert("KeepAlive".into(), plist::Value::Dictionary(keep_alive));
  dictionary.insert("ThrottleInterval".into(), 10_u64.into());
  for (key, file) in [
    ("StandardOutPath", "agentd.stdout.log"),
    ("StandardErrorPath", "agentd.stderr.log"),
  ] {
    dictionary.insert(
      key.into(),
      registration
        .data_dir
        .join(file)
        .to_str()
        .context("log path must be UTF-8")?
        .into(),
    );
  }
  let mut bytes = Vec::new();
  plist::Value::Dictionary(dictionary).to_writer_xml(&mut bytes)?;
  String::from_utf8(bytes).context("native plist serialization was not UTF-8")
}

pub fn systemd_definition(registration: &Registration) -> Result<String> {
  let mut args = vec![registration.executable.as_os_str().to_owned()];
  args.extend(arguments(registration));
  let command = args
    .iter()
    .map(|arg| systemd_quote(arg.to_str().context("service paths must be UTF-8")?))
    .collect::<Result<Vec<_>>>()?
    .join(" ");
  let cwd = systemd_quote(
    registration
      .data_dir
      .to_str()
      .context("data directory must be UTF-8")?,
  )?;
  Ok(format!(
    "[Unit]\nDescription=Lyonbrown per-user agent daemon\n\n[Service]\nType=simple\nExecStart={command}\nWorkingDirectory={cwd}\nRestart=on-failure\nRestartSec=5\nTimeoutStopSec=30\nUMask=0077\n\n[Install]\nWantedBy=default.target\n"
  ))
}

fn systemd_quote(value: &str) -> Result<String> {
  ensure!(
    !value.chars().any(|c| c == '\n' || c == '\r' || c == '\0'),
    "invalid newline or NUL in service argument"
  );
  Ok(format!(
    "\"{}\"",
    value
      .replace('\\', "\\\\")
      .replace('"', "\\\"")
      .replace('%', "%%")
      .replace('$', "$$")
  ))
}

#[cfg(not(windows))]
impl NativeService for NativeManager {
  fn status(&self) -> Result<ServiceState> {
    // Checking the file separately distinguishes unloaded registration from absence.
    let file_exists = match fs::metadata(definition_path()?) {
      Ok(_) => true,
      Err(error) if error.kind() == ErrorKind::NotFound => false,
      Err(error) => return Err(error.into()),
    };
    match manager()?.status(ServiceStatusCtx { label: label()? })? {
      ServiceStatus::Running => Ok(ServiceState::Running),
      ServiceStatus::Stopped(_) => Ok(ServiceState::Stopped),
      ServiceStatus::NotInstalled if file_exists => Ok(ServiceState::Stopped),
      ServiceStatus::NotInstalled => Ok(ServiceState::Absent),
    }
  }

  fn validate(&self, registration: &Registration) -> Result<()> {
    let actual = fs::read_to_string(definition_path()?)
      .context("read registered native service definition")?;
    let expected = definition(registration)?;
    if cfg!(target_os = "macos") {
      let actual = plist::Value::from_reader_xml(actual.as_bytes())?;
      let expected = plist::Value::from_reader_xml(expected.as_bytes())?;
      ensure!(
        actual == expected,
        "native service definition differs from managed registration"
      );
    } else {
      ensure!(
        actual == expected,
        "native service definition differs from managed registration"
      );
    }
    Ok(())
  }

  fn install(&self, registration: &Registration) -> Result<()> {
    manager()?.install(ServiceInstallCtx {
      label: label()?,
      program: registration.executable.clone(),
      args: arguments(registration),
      contents: Some(definition(registration)?),
      username: None,
      working_directory: Some(registration.data_dir.clone()),
      environment: None,
      autostart: true,
      restart_policy: RestartPolicy::default(),
    })?;
    Ok(())
  }
  fn start(&self) -> Result<()> {
    manager()?.start(ServiceStartCtx { label: label()? })?;
    Ok(())
  }
  fn stop(&self) -> Result<()> {
    manager()?.stop(ServiceStopCtx { label: label()? })?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while self.status()? == ServiceState::Running {
      ensure!(
        Instant::now() < deadline,
        "native daemon did not stop within 30 seconds; refusing replacement"
      );
      std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
  }
  fn uninstall(&self) -> Result<()> {
    manager()?.uninstall(ServiceUninstallCtx { label: label()? })?;
    Ok(())
  }
}
