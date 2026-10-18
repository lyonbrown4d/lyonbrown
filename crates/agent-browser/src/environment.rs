use std::ffi::OsString;

/// Only native platform/runtime discovery variables cross into untrusted page processes.
/// Daemon authentication and model-provider credentials are intentionally absent.
pub fn restricted_environment(
  variables: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
  const ALLOWED: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMP",
    "TEMP",
    "TMPDIR",
    "SystemRoot",
    "WINDIR",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "XDG_RUNTIME_DIR",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "CEF_PATH",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "__CF_USER_TEXT_ENCODING",
    "__CFBundleIdentifier",
  ];
  variables
    .into_iter()
    .filter(|(key, _)| key.to_str().is_some_and(|key| ALLOWED.contains(&key)))
    .collect()
}
