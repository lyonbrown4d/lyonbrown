use crate::{Connection, Plan};
use anyhow::{Context, Result, ensure};
use std::{
  collections::BTreeMap,
  path::{Path, PathBuf},
  process::{Command, Stdio},
};

/// Typed product selection keeps client-only features out of daemon builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductBuild {
  Daemon,
  Desktop { native_computer: bool },
  Tui { native_computer: bool },
}
impl ProductBuild {
  pub fn package(self) -> &'static str {
    match self {
      Self::Daemon => "agentd",
      Self::Desktop { .. } => "agent-desktop",
      Self::Tui { .. } => "agent-cli",
    }
  }
  fn native_computer(self) -> bool {
    matches!(
      self,
      Self::Desktop {
        native_computer: true
      } | Self::Tui {
        native_computer: true
      }
    )
  }
  /// Cargo arguments are passed directly, never through a shell.
  pub fn command(self, root: &Path) -> Command {
    build_command(root, self.package(), self.native_computer())
  }
  pub fn display_command(self) -> String {
    format!(
      "cargo {}",
      build_arguments(self.package(), self.native_computer()).join(" ")
    )
  }
  pub async fn build(self, root: &Path) -> Result<PathBuf> {
    run_build(self.command(root), self.package()).await
  }
}

fn build_arguments(package: &str, native_computer: bool) -> Vec<&str> {
  let mut arguments = vec![
    "build",
    "--locked",
    "-p",
    package,
    "--bin",
    package,
    "--no-default-features",
  ];
  if native_computer {
    arguments.extend(["--features", "native-computer"]);
  }
  arguments
}
fn build_command(root: &Path, package: &str, native_computer: bool) -> Command {
  let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
  command
    .current_dir(root)
    .args(build_arguments(package, native_computer))
    .arg("--message-format=json-render-diagnostics")
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit());
  command
}

/// Build one product at a time: client features must never change the daemon artifact.
pub async fn build_binary(root: &Path, package: &str) -> Result<PathBuf> {
  run_build(build_command(root, package, false), package).await
}
async fn run_build(command: Command, package: &str) -> Result<PathBuf> {
  let mut child = crate::process::spawn_command(command).context("start product build")?;
  let stdout = child.stdout().take().context("read Cargo build output")?;
  let executable = crate::cargo::executable(stdout, package).await;
  ensure!(child.wait().await?.success(), "{package} build failed");
  executable
}

pub fn client_command(
  binary: &Path,
  root: &Path,
  plan: &Plan,
  ambient: &BTreeMap<String, String>,
  connection: &Connection,
) -> Command {
  let mut command = product_command(binary, root, plan, ambient);
  command.env("LYONBROWN_TOKEN", connection.token());
  command
}

pub(crate) fn product_command(
  binary: &Path,
  root: &Path,
  plan: &Plan,
  ambient: &BTreeMap<String, String>,
) -> Command {
  let mut command = Command::new(binary);
  command.current_dir(root).env_clear();
  for name in [
    "LYONBROWN_TOKEN",
    "LYONBROWN_DEV_READY_NONCE",
    "LYONBROWN_ENDPOINT",
    "LYONBROWN_LISTEN",
    "LYONBROWN_CONFIG",
    "LYONBROWN_DATA_DIR",
  ] {
    command.env_remove(name);
  }
  command.envs(plan.environment(ambient));
  command
}

#[cfg(test)]
#[path = "../tests/internal/build_process.rs"]
mod tests;
