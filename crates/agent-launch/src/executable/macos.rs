//! Kernel process-image lookup, independent of mutable installation selectors.
use std::{ffi::CStr, os::unix::ffi::OsStringExt, path::PathBuf};

use anyhow::{Context, Result, ensure};

#[link(name = "proc")]
unsafe extern "C" {}

pub(super) fn current_executable() -> Result<PathBuf> {
  let pid = i32::try_from(std::process::id()).context("current process ID exceeds pid_t")?;
  let mut buffer = [0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
  let capacity = u32::try_from(buffer.len()).context("process path buffer exceeds u32")?;
  // SAFETY: pid identifies this live process. The writable buffer has exactly
  // capacity bytes and remains alive throughout the synchronous call; libproc
  // does not retain its pointer. Only the current process's image is queried.
  let length = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), capacity) };
  if length <= 0 {
    return Err(std::io::Error::last_os_error()).context("locate loaded client executable");
  }
  let path = CStr::from_bytes_until_nul(&buffer).context("unterminated process image path")?;
  let executable = PathBuf::from(std::ffi::OsString::from_vec(path.to_bytes().to_vec()));
  ensure!(
    executable.is_absolute(),
    "loaded process image path is not absolute"
  );
  Ok(executable)
}
