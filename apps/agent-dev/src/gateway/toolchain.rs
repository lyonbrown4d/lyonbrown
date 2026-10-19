use super::{
  process,
  tools::{find_tool, tool_command},
};
use anyhow::{Context, Result, ensure};
use std::{
  collections::BTreeMap,
  ffi::{OsStr, OsString},
  path::{Path, PathBuf},
  process::Command,
};

const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// Paths reported by rustup for the active repository toolchain, shared by every build child.
pub struct PreparedToolchain {
  cargo: PathBuf,
  rustc: PathBuf,
  rustdoc: PathBuf,
}
impl PreparedToolchain {
  pub fn new(cargo: PathBuf, rustc: PathBuf, rustdoc: PathBuf) -> Result<Self> {
    ensure!(
      [&cargo, &rustc, &rustdoc]
        .into_iter()
        .all(|path| path.is_absolute() && path.file_name().is_some()),
      "rustup must report absolute tool executable paths"
    );
    Ok(Self {
      cargo,
      rustc,
      rustdoc,
    })
  }
  pub fn cargo_command(
    &self,
    root: &Path,
    ambient: &BTreeMap<OsString, OsString>,
  ) -> Result<Command> {
    self.command(&self.cargo, root, ambient)
  }
  pub fn command(
    &self,
    program: &Path,
    root: &Path,
    ambient: &BTreeMap<OsString, OsString>,
  ) -> Result<Command> {
    let mut command = tool_command(program, root, ambient);
    command
      .env("CARGO", &self.cargo)
      .env("RUSTC", &self.rustc)
      .env("RUSTDOC", &self.rustdoc);
    // Some upstream tools spawn `cargo` by name. Only their child PATH is changed.
    let bin = self
      .cargo
      .parent()
      .context("rustup Cargo executable has no parent directory")?;
    let path = ambient
      .get(OsStr::new("PATH"))
      .or_else(|| ambient.get(OsStr::new("Path")));
    let mut directories = vec![bin.to_owned()];
    if let Some(path) = path {
      directories.extend(std::env::split_paths(path));
    }
    command.env(
      "PATH",
      std::env::join_paths(directories).context("cannot construct Rust toolchain child PATH")?,
    );
    Ok(command)
  }
}

pub(super) async fn prepare_wasm(
  root: &Path,
  ambient: &BTreeMap<OsString, OsString>,
) -> Result<PreparedToolchain> {
  let rustup = find_tool("rustup", ambient)?;
  let mut list = tool_command(&rustup, root, ambient);
  list.args(["target", "list", "--installed"]);
  let output = process::output(list).await?;
  let installed =
    std::str::from_utf8(&output.stdout).context("rustup target output is not UTF-8")?;
  if !installed.lines().any(|line| line.trim() == WASM_TARGET) {
    let mut add = tool_command(&rustup, root, ambient);
    add.args(["target", "add", WASM_TARGET]);
    process::status(add)
      .await
      .context("prepare WASM target for the current Rust toolchain")?;
  }
  let cargo = which(&rustup, root, ambient, "cargo").await?;
  let rustc = which(&rustup, root, ambient, "rustc").await?;
  let rustdoc = which(&rustup, root, ambient, "rustdoc").await?;
  PreparedToolchain::new(cargo, rustc, rustdoc)
}

async fn which(
  rustup: &Path,
  root: &Path,
  ambient: &BTreeMap<OsString, OsString>,
  tool: &str,
) -> Result<PathBuf> {
  let mut command = tool_command(rustup, root, ambient);
  command.args(["which", tool]);
  let output = process::output(command).await?;
  let path = std::str::from_utf8(&output.stdout)
    .context("rustup executable path must be UTF-8")?
    .trim();
  ensure!(
    !path.is_empty(),
    "rustup did not report a tool executable path"
  );
  Ok(PathBuf::from(path))
}
