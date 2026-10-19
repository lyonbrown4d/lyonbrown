use super::{GatewayPlan, PreparedToolchain, process};
use anyhow::{Context, Result, ensure};
use std::{
  collections::BTreeMap,
  ffi::{OsStr, OsString},
  path::{Path, PathBuf},
  process::Command,
};

pub const CARGO_LEPTOS_VERSION: &str = "0.3.11";

/// Tool processes receive toolchain settings, never the Gateway's runtime credentials.
pub fn tool_command(
  program: &Path,
  root: &Path,
  ambient: &BTreeMap<OsString, OsString>,
) -> Command {
  let mut command = Command::new(program);
  command.current_dir(root).env_clear();
  for (key, value) in ambient {
    if key.to_str().is_some_and(tool_setting) {
      command.env(key, value);
    }
  }
  command
}
fn tool_setting(key: &str) -> bool {
  key.starts_with("CARGO_")
    || key.starts_with("RUSTUP_")
    || key.starts_with("RUST_")
    || key.starts_with("PKG_CONFIG_")
    || key.starts_with("CMAKE_")
    || key.starts_with("CC_")
    || key.starts_with("CXX_")
    || matches!(
      key,
      "PATH"
        | "Path"
        | "PATHEXT"
        | "HOME"
        | "USERPROFILE"
        | "HOMEDRIVE"
        | "HOMEPATH"
        | "SystemRoot"
        | "SYSTEMROOT"
        | "WINDIR"
        | "TEMP"
        | "TMP"
        | "TMPDIR"
        | "LOCALAPPDATA"
        | "APPDATA"
        | "XDG_CACHE_HOME"
        | "RUSTFLAGS"
        | "RUSTDOCFLAGS"
        | "RUSTC"
        | "RUSTDOC"
        | "RUSTC_WRAPPER"
        | "RUSTC_WORKSPACE_WRAPPER"
        | "CC"
        | "CXX"
        | "AR"
        | "LD"
        | "LDFLAGS"
        | "CFLAGS"
        | "CXXFLAGS"
        | "SDKROOT"
        | "MACOSX_DEPLOYMENT_TARGET"
        | "LIBCLANG_PATH"
        | "SSL_CERT_FILE"
        | "SSL_CERT_DIR"
        | "CARGO"
    )
}
pub(super) fn find_tool(name: &str, ambient: &BTreeMap<OsString, OsString>) -> Result<PathBuf> {
  let executable = format!("{name}{}", std::env::consts::EXE_SUFFIX);
  let path = ambient
    .get(OsStr::new("PATH"))
    .or_else(|| ambient.get(OsStr::new("Path")));
  if let Some(path) = path {
    for directory in std::env::split_paths(path) {
      let candidate = directory.join(&executable);
      if candidate.is_file() {
        return Ok(candidate);
      }
    }
  }
  if let Some(home) = ambient
    .get(OsStr::new("HOME"))
    .or_else(|| ambient.get(OsStr::new("USERPROFILE")))
  {
    let candidate = Path::new(home).join(".cargo/bin").join(executable);
    if candidate.is_file() {
      return Ok(candidate);
    }
  }
  anyhow::bail!(
    "required Rust tool is unavailable: {name}; install Rust or add its bin directory to PATH"
  )
}
pub(super) async fn prepare_leptos(
  plan: &GatewayPlan,
  toolchain: &PreparedToolchain,
  ambient: &BTreeMap<OsString, OsString>,
) -> Result<PathBuf> {
  let executable = plan
    .tools_root
    .join("bin")
    .join(format!("cargo-leptos{}", std::env::consts::EXE_SUFFIX));
  if !executable.is_file() {
    let mut install = toolchain.cargo_command(&plan.root, ambient)?;
    install
      .args(["install", "--locked", "--root"])
      .arg(&plan.tools_root)
      .args(["cargo-leptos", "--version", CARGO_LEPTOS_VERSION]);
    process::status(install)
      .await
      .context("install repository-local cargo-leptos")?;
  }
  let mut version = toolchain.command(&executable, &plan.root, ambient)?;
  version.args(["leptos", "--version"]);
  let output = process::output(version).await?;
  let output =
    std::str::from_utf8(&output.stdout).context("cargo-leptos version output is not UTF-8")?;
  let mut parts = output.split_whitespace();
  ensure!(
    matches!(
      parts.next(),
      Some("cargo-leptos" | "cargo-leptos-leptos" | "leptos")
    ) && parts.next() == Some(CARGO_LEPTOS_VERSION)
      && parts.next().is_none(),
    "repository-local cargo-leptos version differs from the required 0.3.11"
  );
  Ok(executable)
}
