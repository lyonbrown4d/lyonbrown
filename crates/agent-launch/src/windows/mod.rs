//! Windows SCM adapter. The service runs as the installing Windows user.
mod credentials;
mod identity;
mod protection;
mod provisioning;
mod setup;
pub use protection::{daemon_directory, validate_executable};
pub use setup::{
  current_user_sid, elevate_service_setup, setup_is_elevated, validate_expected_user_sid,
};

use anyhow::{Context, Result, bail, ensure};
use std::{
  ffi::OsStr,
  time::{Duration, Instant},
};
use windows_service::{
  service::{Service, ServiceAccess, ServiceStartType, ServiceState as ScmState, ServiceType},
  service_manager::{ServiceManager, ServiceManagerAccess},
};
use windows_sys::Win32::{
  Foundation::{ERROR_ACCESS_DENIED, ERROR_SERVICE_DOES_NOT_EXIST},
  System::Services::{ChangeServiceConfigW, SERVICE_NO_CHANGE},
};

use crate::{
  model::{NativeService, Registration, ServiceState},
  native::NativeManager,
};
use identity::{Identity, wide};

/// Expected setup failures can be handled without parsing OS error text.
#[derive(Debug)]
pub enum WindowsSetupError {
  ElevationRequired,
  CredentialsCancelled,
  IdentityMismatch,
  SetupUserMismatch,
  ElevationCancelled,
}
impl std::fmt::Display for WindowsSetupError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(match self {
      Self::ElevationRequired => "Windows SCM setup requires elevation under the same Windows account; run the native Lyonbrown installer/service setup and accept UAC",
      Self::CredentialsCancelled => "Windows service credential entry was cancelled; service was not installed",
      Self::IdentityMismatch => "Windows service account differs from the current user; refusing registration or replacement",
      Self::SetupUserMismatch => "Windows account changed during service setup; alternate-administrator UAC cannot register the original user service",
      Self::ElevationCancelled => "Windows service setup elevation was cancelled; service registration was not changed",
    })
  }
}
impl std::error::Error for WindowsSetupError {}

fn scm_error(error: windows_service::Error) -> anyhow::Error {
  if matches!(&error, windows_service::Error::Winapi(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32))
  {
    WindowsSetupError::ElevationRequired.into()
  } else {
    error.into()
  }
}

fn open(access: ServiceAccess) -> Result<Service> {
  let identity = Identity::current()?;
  ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
    .map_err(scm_error)?
    .open_service(identity.service_name(), access)
    .map_err(scm_error)
}

fn account_matches(service: &Service, identity: &Identity) -> Result<()> {
  let config = service.query_config()?;
  let name = config
    .account_name
    .context("SCM service has no account identity")?;
  ensure!(
    identity.matches_account(&name)?,
    WindowsSetupError::IdentityMismatch
  );
  ensure!(
    config.service_type == ServiceType::OWN_PROCESS,
    "registered daemon is not an own-process SCM service"
  );
  Ok(())
}

/// Password-free SCM command; paths are validated before any service mutation.
pub fn command_line(registration: &Registration, service_name: &str) -> Result<Vec<u16>> {
  ensure!(
    registration.executable.is_absolute() && registration.executable.is_file(),
    "SCM daemon executable must be an existing absolute file"
  );
  ensure!(
    registration.data_dir.is_absolute() && registration.data_dir.is_dir(),
    "SCM data directory must be an existing absolute directory"
  );
  ensure!(
    service_name.starts_with("Lyonbrown.Agentd.")
      && service_name[17..].chars().all(|c| c.is_ascii_hexdigit())
      && service_name.len() == 81,
    "invalid SCM service identity"
  );
  let mut command = quote(registration.executable.as_os_str())?;
  command.push_str(" --windows-service ");
  command.push_str(service_name);
  command.push_str(" --listen 127.0.0.1:7447 --data-dir ");
  command.push_str(&quote(registration.data_dir.as_os_str())?);
  wide(OsStr::new(&command))
}

fn quote(value: &OsStr) -> Result<String> {
  let value = value
    .to_str()
    .context("SCM service paths must be valid Unicode")?;
  ensure!(
    !value.chars().any(|c| matches!(c, '"' | '\0' | '\n' | '\r')),
    "invalid character in SCM path"
  );
  // Windows filenames cannot contain quotes; duplicate trailing backslashes before closing quote.
  let trailing = value.chars().rev().take_while(|c| *c == '\\').count();
  Ok(format!("\"{value}{}\"", "\\".repeat(trailing)))
}

impl NativeService for NativeManager {
  fn status(&self) -> Result<ServiceState> {
    let service = match open(ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG) {
      Ok(service) => service,
      Err(error) if error.downcast_ref::<windows_service::Error>().is_some_and(|e| matches!(e, windows_service::Error::Winapi(io) if io.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32))) => return Ok(ServiceState::Absent),
      Err(error) => return Err(error),
    };
    account_matches(&service, &Identity::current()?)?;
    match service.query_status()?.current_state {
      ScmState::Stopped => Ok(ServiceState::Stopped),
      ScmState::Running => Ok(ServiceState::Running),
      state => bail!("SCM service is in transitional state {state:?}; wait and retry"),
    }
  }
  fn validate(&self, registration: &Registration) -> Result<()> {
    protection::validate_executable(registration)?;
    let identity = Identity::current()?;
    let service = open(ServiceAccess::QUERY_CONFIG)?;
    account_matches(&service, &identity)?;
    let config = service.query_config()?;
    let expected = command_line(registration, &identity.service_name())?;
    use std::os::windows::ffi::OsStrExt;
    let actual: Vec<_> = config
      .executable_path
      .as_os_str()
      .encode_wide()
      .chain(Some(0))
      .collect();
    ensure!(
      actual == expected && config.start_type == ServiceStartType::AutoStart,
      "SCM service definition differs from managed registration"
    );
    Ok(())
  }
  fn install(&self, registration: &Registration) -> Result<()> {
    provisioning::install(registration)
  }
  fn update(&self, registration: &Registration) -> Result<()> {
    protection::validate_executable(registration)?;
    let identity = Identity::current()?;
    let command = command_line(registration, &identity.service_name())?;
    let service = open(
      ServiceAccess::CHANGE_CONFIG | ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS,
    )?;
    account_matches(&service, &identity)?;
    ensure!(
      service.query_status()?.current_state == ScmState::Stopped,
      "SCM service must be stopped before replacement"
    );
    // SAFETY: service handle is live; command is terminated UTF-16. NULL preserves
    // account, LSA-held password, dependencies and all unmodified service settings.
    let changed = unsafe {
      ChangeServiceConfigW(
        service.raw_handle().cast(),
        SERVICE_NO_CHANGE,
        SERVICE_NO_CHANGE,
        SERVICE_NO_CHANGE,
        command.as_ptr(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
      )
    };
    identity::check(changed).context("update SCM executable without replacing service credentials")
  }
  fn start(&self) -> Result<()> {
    let service = open(ServiceAccess::START | ServiceAccess::QUERY_CONFIG)?;
    account_matches(&service, &Identity::current()?)?;
    service.start::<&str>(&[]).map_err(scm_error)
  }
  fn stop(&self) -> Result<()> {
    let service =
      open(ServiceAccess::STOP | ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG)?;
    account_matches(&service, &Identity::current()?)?;
    service.stop().map_err(scm_error)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while service.query_status()?.current_state != ScmState::Stopped {
      ensure!(
        Instant::now() < deadline,
        "SCM daemon did not stop within 30 seconds; refusing replacement"
      );
      std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
  }
  fn uninstall(&self) -> Result<()> {
    let service =
      open(ServiceAccess::DELETE | ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS)?;
    account_matches(&service, &Identity::current()?)?;
    ensure!(
      service.query_status()?.current_state == ScmState::Stopped,
      "SCM service must be stopped before removal"
    );
    service.delete().map_err(scm_error)
  }
}
