//! Reject executable paths that an unelevated service owner can replace.
use super::identity::{check, wide};
use crate::model::Registration;
use anyhow::{Context, Result, ensure};
use std::{
  ffi::OsString,
  mem::size_of,
  os::windows::{
    ffi::OsStringExt,
    io::{AsRawHandle, FromRawHandle, OwnedHandle},
  },
  path::{Path, PathBuf},
};
use windows_sys::Win32::{
  Foundation::{HANDLE, LocalFree},
  Security::{
    AccessCheck,
    Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
    DACL_SECURITY_INFORMATION, DuplicateToken, GENERIC_MAPPING, GROUP_SECURITY_INFORMATION,
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, SecurityImpersonation,
    TOKEN_DUPLICATE, TOKEN_LINKED_TOKEN, TOKEN_QUERY, TokenElevationType, TokenElevationTypeFull,
    TokenLinkedToken,
  },
  Storage::FileSystem::{
    DELETE, FILE_ALL_ACCESS, FILE_APPEND_DATA, FILE_DELETE_CHILD, FILE_GENERIC_EXECUTE,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA,
    WRITE_DAC, WRITE_OWNER,
  },
  System::{
    Com::CoTaskMemFree,
    SystemServices::MAXIMUM_ALLOWED,
    Threading::{GetCurrentProcess, OpenProcessToken},
  },
  UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath},
};

/// Machine installation root from the Windows known-folder API, never environment input.
pub fn daemon_directory() -> Result<PathBuf> {
  Ok(program_files()?.join("Lyonbrown").join("Daemon"))
}

fn program_files() -> Result<PathBuf> {
  let mut output = std::ptr::null_mut();
  // SAFETY: static known-folder GUID, default token, valid CoTaskMem-owned output.
  let status =
    unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &mut output) };
  ensure!(
    status >= 0,
    "resolve trusted Windows Program Files directory failed ({status:#x})"
  );
  let mut length = 0;
  // SAFETY: successful known-folder call returns a NUL-terminated UTF-16 string.
  unsafe {
    while *output.add(length) != 0 {
      length += 1;
    }
  }
  // SAFETY: length was found within the returned terminated allocation.
  let path = unsafe {
    PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
      output, length,
    )))
  };
  // SAFETY: SHGetKnownFolderPath transfers ownership to the caller.
  unsafe {
    CoTaskMemFree(output.cast());
  }
  Ok(path)
}

/// Preflight machine path and ACL checks before credential UI or SCM mutation.
pub fn validate_executable(registration: &Registration) -> Result<()> {
  let program_files = program_files()?.canonicalize()?;
  let expected = program_files
    .join("Lyonbrown")
    .join("Daemon")
    .join(registration.version.to_string())
    .join("agentd.exe");
  let actual = registration
    .executable
    .canonicalize()
    .context("resolve SCM daemon executable")?;
  ensure!(
    actual == expected,
    "SCM executable must be the protected Program Files/Lyonbrown/Daemon/<version>/agentd.exe; rerun the native installer"
  );
  validate_machine_file(&actual, &program_files)
}

pub(super) fn validate_setup_executable(executable: &Path) -> Result<PathBuf> {
  let program_files = program_files()?.canonicalize()?;
  let actual = executable
    .canonicalize()
    .context("resolve native service setup executable")?;
  let root = program_files.join("Lyonbrown");
  let version = env!("CARGO_PKG_VERSION");
  ensure!(
    ["CLI", "Desktop"]
      .iter()
      .any(|client| actual == root.join(client).join(version).join("agent-install.exe")),
    "Windows service setup must use the protected installed Program Files/Lyonbrown/<client>/<version>/agent-install.exe"
  );
  validate_machine_file(&actual, &program_files)?;
  Ok(actual)
}

fn validate_machine_file(actual: &Path, program_files: &Path) -> Result<()> {
  ensure!(
    actual.is_file(),
    "protected Windows machine executable is missing"
  );
  let token = unelevated_impersonation_token()?;
  let mut path = actual;
  loop {
    deny_user_write(path, token.as_raw_handle(), None)?;
    if path == program_files {
      break;
    }
    path = path
      .parent()
      .context("SCM executable escaped the trusted machine directory")?;
  }
  if let Some(parent) = program_files.parent() {
    // Creating unrelated files in the drive root is harmless; deleting/replacing
    // Program Files through parent delete-child or ACL ownership is not.
    deny_user_write(
      parent,
      token.as_raw_handle(),
      Some(FILE_DELETE_CHILD | WRITE_DAC | WRITE_OWNER),
    )?;
  }
  Ok(())
}

fn unelevated_impersonation_token() -> Result<OwnedHandle> {
  let mut raw = std::ptr::null_mut();
  // SAFETY: valid process pseudo-handle and writable token output.
  check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut raw) })?;
  // SAFETY: successful OpenProcessToken returned an owned handle.
  let process = unsafe { OwnedHandle::from_raw_handle(raw) };
  let mut elevation = 0_i32;
  let mut size = 0;
  // SAFETY: correctly sized/aligned elevation-type output.
  check(unsafe {
    GetTokenInformation(
      process.as_raw_handle(),
      TokenElevationType,
      (&mut elevation as *mut i32).cast(),
      size_of::<i32>() as u32,
      &mut size,
    )
  })?;
  let source = if elevation == TokenElevationTypeFull {
    let mut linked = TOKEN_LINKED_TOKEN::default();
    // SAFETY: sized/aligned linked-token output; API returns an owned handle.
    check(unsafe {
      GetTokenInformation(
        process.as_raw_handle(),
        TokenLinkedToken,
        (&mut linked as *mut TOKEN_LINKED_TOKEN).cast(),
        size_of::<TOKEN_LINKED_TOKEN>() as u32,
        &mut size,
      )
    })?;
    // SAFETY: GetTokenInformation transfers the returned linked token handle.
    unsafe { OwnedHandle::from_raw_handle(linked.LinkedToken) }
  } else {
    process
  };
  let mut duplicate = std::ptr::null_mut();
  // SAFETY: live primary token and writable owned impersonation-token output.
  check(unsafe {
    DuplicateToken(
      source.as_raw_handle(),
      SecurityImpersonation,
      &mut duplicate,
    )
  })?;
  // SAFETY: successful DuplicateToken returned an owned handle.
  Ok(unsafe { OwnedHandle::from_raw_handle(duplicate) })
}

fn deny_user_write(path: &Path, token: HANDLE, specific: Option<u32>) -> Result<()> {
  let name = wide(path.as_os_str())?;
  let mut descriptor = std::ptr::null_mut();
  // AccessCheck requires owner and group as well as DACL information.
  // SAFETY: terminated path and writable LocalAlloc-owned descriptor output.
  let error = unsafe {
    GetNamedSecurityInfoW(
      name.as_ptr(),
      SE_FILE_OBJECT,
      OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
      std::ptr::null_mut(),
      std::ptr::null_mut(),
      std::ptr::null_mut(),
      std::ptr::null_mut(),
      &mut descriptor,
    )
  };
  if error != 0 {
    return Err(std::io::Error::from_raw_os_error(error as i32).into());
  }
  let mapping = GENERIC_MAPPING {
    GenericRead: FILE_GENERIC_READ,
    GenericWrite: FILE_GENERIC_WRITE,
    GenericExecute: FILE_GENERIC_EXECUTE,
    GenericAll: FILE_ALL_ACCESS,
  };
  let mut privileges = vec![0_usize; 256];
  let mut bytes = (privileges.len() * size_of::<usize>()) as u32;
  let (mut granted, mut allowed) = (0_u32, 0_i32);
  // SAFETY: initialized descriptor; valid impersonation token; pointer-aligned output
  // buffer is 2048 bytes, exceeding the bounded token privilege set on Windows.
  let result = check(unsafe {
    AccessCheck(
      descriptor,
      token,
      MAXIMUM_ALLOWED,
      &mapping,
      privileges.as_mut_ptr().cast::<PRIVILEGE_SET>(),
      &mut bytes,
      &mut granted,
      &mut allowed,
    )
  });
  // SAFETY: descriptor came from GetNamedSecurityInfoW and is no longer referenced.
  unsafe {
    LocalFree(descriptor);
  }
  result.context("check protected SCM executable ACL")?;
  let mut writes = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER;
  if path.is_dir() {
    writes |= FILE_DELETE_CHILD;
  }
  let writes = specific.unwrap_or(writes);
  ensure!(
    allowed == 0 || granted & writes == 0,
    "SCM executable or ancestor is writable by the unelevated current user; repair machine installation ACLs: {}",
    path.display()
  );
  Ok(())
}
