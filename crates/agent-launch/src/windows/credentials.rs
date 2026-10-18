use super::{
  WindowsSetupError,
  identity::{Identity, check, wide},
};
use anyhow::{Result, ensure};
use std::{
  ffi::OsString,
  mem::size_of,
  os::windows::{
    ffi::{OsStrExt, OsStringExt},
    io::{AsRawHandle, FromRawHandle, OwnedHandle},
  },
};
use windows_sys::Win32::{
  Foundation::ERROR_CANCELLED,
  Security::{
    Credentials::{
      CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST, CREDUI_FLAGS_GENERIC_CREDENTIALS,
      CREDUI_FLAGS_KEEP_USERNAME, CREDUI_INFOW, CredUIPromptForCredentialsW,
    },
    LOGON32_LOGON_NETWORK, LOGON32_PROVIDER_DEFAULT, LogonUserW,
  },
};
use zeroize::Zeroizing;

// Deliberately has no Debug, Clone or serialization. The one allocation is zeroed on drop.
pub(super) struct Credentials {
  password: Zeroizing<Vec<u16>>,
}
impl Credentials {
  pub fn prompt(identity: &Identity) -> Result<Self> {
    let mut username = vec![0; 514];
    let account: Vec<_> = identity.account.encode_wide().collect();
    ensure!(
      account.len() < username.len(),
      "Windows account name exceeds native credential UI capacity"
    );
    username[..account.len()].copy_from_slice(&account);
    let mut password = Zeroizing::new(vec![0; 257]);
    let caption = wide("Lyonbrown service setup".as_ref())?;
    let message = wide("Enter this Windows account's password to run agentd as your account. Windows SCM stores the service logon credential.".as_ref())?;
    let target = wide(identity.service_name().as_ref())?;
    let info = CREDUI_INFOW {
      cbSize: size_of::<CREDUI_INFOW>() as u32,
      hwndParent: std::ptr::null_mut(),
      pszMessageText: message.as_ptr(),
      pszCaptionText: caption.as_ptr(),
      hbmBanner: std::ptr::null_mut(),
    };
    let mut save = 0;
    // SAFETY: all text is terminated; mutable buffers have declared capacities.
    // DO_NOT_PERSIST forbids Credential Manager storage; only SCM receives the password.
    let code = unsafe {
      CredUIPromptForCredentialsW(
        &info,
        target.as_ptr(),
        std::ptr::null(),
        0,
        username.as_mut_ptr(),
        username.len() as u32,
        password.as_mut_ptr(),
        password.len() as u32,
        &mut save,
        CREDUI_FLAGS_ALWAYS_SHOW_UI
          | CREDUI_FLAGS_DO_NOT_PERSIST
          | CREDUI_FLAGS_GENERIC_CREDENTIALS
          | CREDUI_FLAGS_KEEP_USERNAME,
      )
    };
    if code == ERROR_CANCELLED {
      return Err(WindowsSetupError::CredentialsCancelled.into());
    }
    if code != 0 {
      return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    let length = username
      .iter()
      .position(|c| *c == 0)
      .unwrap_or(username.len());
    ensure!(
      identity.matches_account(&OsString::from_wide(&username[..length]))?,
      WindowsSetupError::IdentityMismatch
    );
    ensure!(
      password[0] != 0,
      "Windows service logon requires an account password; passwordless/PIN-only sign-in cannot provision an ordinary account service"
    );
    let result = Self { password };
    result.validate(identity)?;
    Ok(result)
  }
  fn validate(&self, identity: &Identity) -> Result<()> {
    let account = identity.account.to_string_lossy();
    let (domain, user) = account
      .split_once('\\')
      .ok_or(WindowsSetupError::IdentityMismatch)?;
    let domain = wide(domain.as_ref())?;
    let user = wide(user.as_ref())?;
    let mut token = std::ptr::null_mut();
    // SAFETY: all inputs are terminated UTF-16 and remain live; output owns one token.
    check(unsafe {
      LogonUserW(
        user.as_ptr(),
        domain.as_ptr(),
        self.password.as_ptr(),
        LOGON32_LOGON_NETWORK,
        LOGON32_PROVIDER_DEFAULT,
        &mut token,
      )
    })?;
    // SAFETY: successful LogonUserW returned an owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    ensure!(
      identity.matches_token(token.as_raw_handle())?,
      WindowsSetupError::IdentityMismatch
    );
    Ok(())
  }
  pub fn password(&self) -> *const u16 {
    self.password.as_ptr()
  }
}
