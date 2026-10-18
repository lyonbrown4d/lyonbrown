use crate::{BrowserError, BrowserResult};
use std::{
  fs::{self, File, OpenOptions},
  path::Path,
  process::Stdio,
};

/// Native engine diagnostics never inherit the TUI's stderr/terminal descriptor.
pub(crate) fn stderr(profiles: &Path) -> BrowserResult<Stdio> {
  fn open(profiles: &Path) -> std::io::Result<File> {
    let logs = profiles.join("runtime-logs");
    fs::create_dir_all(&logs)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&logs, fs::Permissions::from_mode(0o700))?;
    }
    let path = logs.join("native.stderr.log");
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > 8 * 1024 * 1024) {
      fs::rename(&path, logs.join("native.stderr.previous.log"))?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    options.open(path)
  }
  open(profiles).map(Stdio::from).map_err(|error| {
    BrowserError::Unavailable(format!("cannot open private browser diagnostics: {error}"))
  })
}
