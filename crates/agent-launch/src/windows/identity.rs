use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
  ffi::{OsStr, OsString},
  mem::size_of,
  os::windows::{
    ffi::{OsStrExt, OsStringExt},
    io::{AsRawHandle, FromRawHandle, OwnedHandle},
  },
};
use windows_sys::Win32::{
  Foundation::{GetLastError, HANDLE, LocalFree},
  Security::{
    Authorization::ConvertSidToStringSidW, CopySid, EqualSid, GetLengthSid, GetTokenInformation,
    IsWellKnownSid, LookupAccountNameW, LookupAccountSidW, PSID, TOKEN_QUERY, TOKEN_USER,
    TokenUser, WinLocalServiceSid, WinLocalSystemSid, WinNetworkServiceSid,
  },
  System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) struct Identity {
  sid: Vec<u32>,
  pub account: OsString,
}
impl Identity {
  pub fn current() -> Result<Self> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: valid process pseudo-handle and writable token handle output.
    check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) })?;
    // SAFETY: OpenProcessToken returned an owned, valid handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let sid = token_sid(token.as_raw_handle())?;
    // SAFETY: copied token SID is valid. Built-in service accounts cannot identify the installing user.
    ensure!(
      ![WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid]
        .iter()
        .any(|kind| unsafe { IsWellKnownSid(sid.as_ptr().cast_mut().cast(), *kind) } != 0),
      "Windows service setup must run under the installing user, not a built-in service account"
    );
    let mut name = vec![0; 257];
    let mut domain = vec![0; 257];
    let (mut names, mut domains, mut kind) = (name.len() as u32, domain.len() as u32, 0);
    // SAFETY: SID storage is aligned and valid; all output buffers match their capacities.
    check(unsafe {
      LookupAccountSidW(
        std::ptr::null(),
        sid.as_ptr().cast_mut().cast(),
        name.as_mut_ptr(),
        &mut names,
        domain.as_mut_ptr(),
        &mut domains,
        &mut kind,
      )
    })?;
    let mut account = OsString::from_wide(&domain[..domains as usize]);
    account.push("\\");
    account.push(OsString::from_wide(&name[..names as usize]));
    Ok(Self { sid, account })
  }
  pub fn sid(&self) -> PSID {
    self.sid.as_ptr().cast_mut().cast()
  }
  pub fn sid_string(&self) -> Result<String> {
    let mut text = std::ptr::null_mut();
    // SAFETY: valid copied SID; API returns a LocalAlloc-owned terminated string.
    check(unsafe { ConvertSidToStringSidW(self.sid(), &mut text) })?;
    let mut length = 0;
    // SAFETY: successful conversion guarantees a NUL terminator.
    unsafe {
      while *text.add(length) != 0 {
        length += 1;
      }
    }
    // SAFETY: terminator determines the returned allocation's text length.
    let result = unsafe { String::from_utf16(std::slice::from_raw_parts(text, length)) };
    // SAFETY: API allocated this text with LocalAlloc; no references remain.
    unsafe {
      LocalFree(text.cast());
    }
    result.context("Windows returned an invalid SID string")
  }
  pub fn service_name(&self) -> String {
    // Hash canonical SID bytes, not mutable user/account display names.
    let bytes = self
      .sid
      .iter()
      .flat_map(|word| word.to_ne_bytes())
      .collect::<Vec<_>>();
    format!("Lyonbrown.Agentd.{}", hex::encode(Sha256::digest(bytes)))
  }
  pub fn matches_account(&self, account: &OsStr) -> Result<bool> {
    let account = wide(account)?;
    let mut sid = vec![0_u32; 68 / 4];
    let mut domain = vec![0; 257];
    let (mut bytes, mut domains, mut kind) = (68_u32, domain.len() as u32, 0);
    // SAFETY: terminated account, aligned SID buffer and bounded domain output.
    check(unsafe {
      LookupAccountNameW(
        std::ptr::null(),
        account.as_ptr(),
        sid.as_mut_ptr().cast(),
        &mut bytes,
        domain.as_mut_ptr(),
        &mut domains,
        &mut kind,
      )
    })?;
    // SAFETY: LookupAccountNameW returned a valid SID in the output buffer.
    Ok(unsafe { EqualSid(self.sid(), sid.as_mut_ptr().cast()) } != 0)
  }
  pub fn matches_token(&self, token: HANDLE) -> Result<bool> {
    let sid = token_sid(token)?;
    // SAFETY: both buffers contain valid copied SIDs.
    Ok(unsafe { EqualSid(self.sid(), sid.as_ptr().cast_mut().cast()) } != 0)
  }
}

fn token_sid(token: HANDLE) -> Result<Vec<u32>> {
  let mut required = 0;
  // SAFETY: size query only; null output is intentional.
  unsafe {
    GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required);
  }
  ensure!(
    required >= size_of::<TOKEN_USER>() as u32,
    "Windows token SID size query failed"
  );
  let mut buffer = vec![0_usize; (required as usize).div_ceil(size_of::<usize>())];
  // SAFETY: pointer-aligned buffer is at least required bytes; token remains live.
  check(unsafe {
    GetTokenInformation(
      token,
      TokenUser,
      buffer.as_mut_ptr().cast(),
      required,
      &mut required,
    )
  })?;
  // SAFETY: successful GetTokenInformation initialized TOKEN_USER at aligned base.
  let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
  // SAFETY: user SID points into the live token information buffer.
  let length = unsafe { GetLengthSid(user.User.Sid) };
  let mut sid = vec![0_u32; (length as usize).div_ceil(size_of::<u32>())];
  // SAFETY: sized/aligned owned output and valid input SID.
  check(unsafe { CopySid(length, sid.as_mut_ptr().cast(), user.User.Sid) })?;
  Ok(sid)
}

pub(super) fn wide(value: &OsStr) -> Result<Vec<u16>> {
  let mut result: Vec<_> = value.encode_wide().collect();
  ensure!(
    !result.contains(&0),
    "NUL is forbidden in Windows service parameters"
  );
  result.push(0);
  Ok(result)
}
pub(super) fn check(success: i32) -> Result<()> {
  if success != 0 {
    return Ok(());
  }
  // SAFETY: GetLastError has no preconditions; captured immediately after failed call.
  let code = unsafe { GetLastError() };
  if code == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED {
    return Err(super::WindowsSetupError::ElevationRequired.into());
  }
  Err(std::io::Error::from_raw_os_error(code as i32)).context("Windows service API failed")
}
