use crate::{Connection, Plan};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
  collections::BTreeMap,
  path::{Path, PathBuf},
  process::{Command, Stdio},
};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Build one product at a time: client features must never change the daemon artifact.
pub async fn build_binary(root: &Path, package: &str) -> Result<PathBuf> {
  let mut command =
    tokio::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
  command
    .current_dir(root)
    .args([
      "build",
      "--locked",
      "-p",
      package,
      "--bin",
      package,
      "--no-default-features",
      "--message-format=json-render-diagnostics",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .kill_on_drop(true);
  let mut child = command.spawn().context("start product build")?;
  let stdout = child.stdout.take().context("read Cargo build output")?;
  let mut lines = BufReader::new(stdout).lines();
  let mut executable = None;
  while let Some(line) = lines.next_line().await? {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
      && value["reason"] == "compiler-message"
      && let Some(rendered) = value["message"]["rendered"].as_str()
    {
      eprint!("{rendered}");
    }
    if let Ok(message) = serde_json::from_str::<Artifact>(&line)
      && message.reason == "compiler-artifact"
      && message.target.as_ref().is_some_and(|target| {
        target.name == package && target.kind.iter().any(|kind| kind == "bin")
      })
    {
      executable = message.executable;
    }
  }
  ensure!(child.wait().await?.success(), "{package} build failed");
  executable.context("Cargo did not report the product executable")
}
#[derive(Deserialize)]
struct Artifact {
  reason: String,
  target: Option<ArtifactTarget>,
  executable: Option<PathBuf>,
}
#[derive(Deserialize)]
struct ArtifactTarget {
  name: String,
  kind: Vec<String>,
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
