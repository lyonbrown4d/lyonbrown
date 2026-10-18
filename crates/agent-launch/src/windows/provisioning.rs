use super::{
  command_line,
  credentials::Credentials,
  identity::{Identity, check, wide},
};
use crate::model::Registration;
use anyhow::{Context, Result};
use std::{ffi::OsString, mem::size_of, os::windows::ffi::OsStringExt};
use windows_sys::Win32::{
  Foundation::LocalFree,
  Security::{
    Authentication::Identity::{
      LSA_OBJECT_ATTRIBUTES, LSA_UNICODE_STRING, LsaAddAccountRights, LsaClose,
      LsaNtStatusToWinError, LsaOpenPolicy, POLICY_CREATE_ACCOUNT, POLICY_LOOKUP_NAMES,
    },
    Authorization::{
      ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    },
    DACL_SECURITY_INFORMATION,
  },
  System::Services::{
    CloseServiceHandle, CreateServiceW, DeleteService, OpenSCManagerW, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE, SERVICE_ALL_ACCESS, SERVICE_AUTO_START,
    SERVICE_ERROR_NORMAL, SERVICE_WIN32_OWN_PROCESS, SetServiceObjectSecurity,
  },
};

struct ScHandle(SC_HANDLE);
impl Drop for ScHandle {
  fn drop(&mut self) {
    // SAFETY: handle was returned by SCM and is closed exactly once.
    unsafe {
      CloseServiceHandle(self.0);
    }
  }
}

pub(super) fn install(registration: &Registration) -> Result<()> {
  super::protection::validate_executable(registration)?;
  let identity = Identity::current()?;
  let command = command_line(registration, &identity.service_name())?;
  // Check elevation before displaying credential UI. Never start a fallback process.
  // SAFETY: NULL selects the local active SCM database.
  let manager = unsafe {
    OpenSCManagerW(
      std::ptr::null(),
      std::ptr::null(),
      SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE,
    )
  };
  check(i32::from(!manager.is_null()))?;
  let manager = ScHandle(manager);
  let credentials = Credentials::prompt(&identity)?;
  grant_service_logon(&identity)?;
  let name = wide(identity.service_name().as_ref())?;
  let display = wide("Lyonbrown agent daemon (current user)".as_ref())?;
  let account = wide(&identity.account)?;
  // SAFETY: live SCM handle and terminated UTF-16 buffers; password is only borrowed
  // from the zeroizing native UI buffer. SCM stores its own protected logon secret.
  let service = unsafe {
    CreateServiceW(
      manager.0,
      name.as_ptr(),
      display.as_ptr(),
      SERVICE_ALL_ACCESS,
      SERVICE_WIN32_OWN_PROCESS,
      SERVICE_AUTO_START,
      SERVICE_ERROR_NORMAL,
      command.as_ptr(),
      std::ptr::null(),
      std::ptr::null_mut(),
      std::ptr::null(),
      account.as_ptr(),
      credentials.password(),
    )
  };
  // Capture the OS error before destroying the secret buffer (deallocation may
  // otherwise overwrite the calling thread's last-error value).
  let created = check(i32::from(!service.is_null()));
  drop(credentials);
  created.context("create current-account Windows SCM service")?;
  let service = ScHandle(service);
  let configured = set_owner_access(service.0, &identity).and_then(|()| configure_recovery());
  if let Err(error) = configured {
    // SAFETY: only the newly created, never-started service is removed on ACL failure.
    let removed = unsafe { DeleteService(service.0) };
    check(removed).context("SCM ACL setup failed and new service cleanup also failed")?;
    return Err(error);
  }
  Ok(())
}

fn grant_service_logon(identity: &Identity) -> Result<()> {
  let attributes = LSA_OBJECT_ATTRIBUTES {
    Length: size_of::<LSA_OBJECT_ATTRIBUTES>() as u32,
    ..Default::default()
  };
  let mut policy = 0;
  // SAFETY: local policy, valid initialized attributes and writable handle output.
  let status = unsafe {
    LsaOpenPolicy(
      std::ptr::null(),
      &attributes,
      (POLICY_LOOKUP_NAMES | POLICY_CREATE_ACCOUNT) as u32,
      &mut policy,
    )
  };
  lsa_result(status)?;
  let mut name = wide("SeServiceLogonRight".as_ref())?;
  let right = LSA_UNICODE_STRING {
    Length: ((name.len() - 1) * 2) as u16,
    MaximumLength: (name.len() * 2) as u16,
    Buffer: name.as_mut_ptr(),
  };
  // SAFETY: policy, SID and unicode right remain valid for the call.
  let status = unsafe { LsaAddAccountRights(policy, identity.sid(), &right, 1) };
  // SAFETY: policy is an owned LSA handle and is no longer used.
  unsafe {
    LsaClose(policy);
  }
  lsa_result(status)
    .context("grant Log on as a service; domain deny policy may still prohibit service logon")
}

fn lsa_result(status: i32) -> Result<()> {
  if status == 0 {
    return Ok(());
  }
  // SAFETY: NTSTATUS conversion has no pointer preconditions.
  let error = unsafe { LsaNtStatusToWinError(status) };
  if error == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED {
    return Err(super::WindowsSetupError::ElevationRequired.into());
  }
  Err(std::io::Error::from_raw_os_error(error as i32).into())
}

fn set_owner_access(service: SC_HANDLE, identity: &Identity) -> Result<()> {
  let mut sid_text = std::ptr::null_mut();
  // SAFETY: valid SID and writable pointer for LocalAlloc-owned terminated text.
  check(unsafe { ConvertSidToStringSidW(identity.sid(), &mut sid_text) })?;
  let mut length = 0;
  // SAFETY: successful API returns NUL-terminated text.
  unsafe {
    while *sid_text.add(length) != 0 {
      length += 1;
    }
  }
  // SAFETY: length was bounded by the returned valid string terminator.
  let text = unsafe { OsString::from_wide(std::slice::from_raw_parts(sid_text, length)) };
  // SAFETY: ConvertSidToStringSidW allocated this buffer with LocalAlloc.
  unsafe {
    LocalFree(sid_text.cast());
  }
  // Owner can query/start/stop, while executable/account changes remain administrator-only.
  let sddl = wide(
    format!(
      "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x20035;;;{})",
      text.to_string_lossy()
    )
    .as_ref(),
  )?;
  let mut descriptor = std::ptr::null_mut();
  // SAFETY: terminated SDDL and writable descriptor output.
  check(unsafe {
    ConvertStringSecurityDescriptorToSecurityDescriptorW(
      sddl.as_ptr(),
      SDDL_REVISION_1,
      &mut descriptor,
      std::ptr::null_mut(),
    )
  })?;
  // SAFETY: valid service and initialized security descriptor; only its DACL is changed.
  let result =
    check(unsafe { SetServiceObjectSecurity(service, DACL_SECURITY_INFORMATION, descriptor) });
  // SAFETY: descriptor is LocalAlloc-owned and no longer referenced.
  unsafe {
    LocalFree(descriptor);
  }
  result.context("limit Windows service control to its owner and administrators")
}

fn configure_recovery() -> Result<()> {
  use std::time::Duration;
  use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceFailureActions,
    ServiceFailureResetPeriod,
  };
  let service = super::open(ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
  service.update_failure_actions(ServiceFailureActions {
    reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86_400)),
    reboot_msg: None,
    command: None,
    actions: Some(vec![
      ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(5),
      },
      ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(30),
      },
      ServiceAction {
        action_type: ServiceActionType::None,
        delay: Duration::ZERO,
      },
    ]),
  })?;
  service.set_failure_actions_on_non_crash_failures(true)?;
  Ok(())
}
