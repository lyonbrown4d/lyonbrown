//! Repository-local full-stack Gateway development tooling. No daemon service is started.
mod build;
mod process;
mod toolchain;
mod tools;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{
  collections::BTreeMap,
  ffi::OsString,
  net::SocketAddr,
  path::{Path, PathBuf},
  process::Command,
};
pub use toolchain::PreparedToolchain;
pub use tools::{CARGO_LEPTOS_VERSION, tool_command};

#[derive(Parser)]
#[command(about = "Build the Rust Gateway WebUI and supervise its development server")]
pub struct GatewayOptions {
  /// Gateway TOML; administrator credentials come from its adjacent .env or process environment.
  #[arg(long, env = "LYONBROWN_GATEWAY_CONFIG")]
  pub config: Option<PathBuf>,
  /// Gateway development state, relative to the repository root.
  #[arg(long, env = "LYONBROWN_GATEWAY_DATA_DIR")]
  pub data_dir: Option<PathBuf>,
  #[arg(long)]
  pub listen: Option<SocketAddr>,
  #[arg(long)]
  pub release: bool,
  /// Build server and hydration assets without starting the Gateway.
  #[arg(long)]
  pub build_only: bool,
  /// Print the plan without executing tools or creating files.
  #[arg(long)]
  pub dry_run: bool,
}

pub struct GatewayPlan {
  pub root: PathBuf,
  pub tools_root: PathBuf,
  pub web_root: PathBuf,
  pub data_dir: PathBuf,
  pub config: Option<PathBuf>,
  pub listen: Option<SocketAddr>,
  pub release: bool,
}
impl GatewayPlan {
  pub fn new(root: &Path, options: &GatewayOptions) -> Result<Self> {
    ensure!(root.is_absolute(), "repository root must be absolute");
    if let Some(listen) = options.listen {
      ensure!(
        listen.ip().is_loopback() && listen.port() != 0,
        "gateway requires a nonzero loopback listener"
      );
    }
    Ok(Self {
      root: root.to_owned(),
      tools_root: root.join(".local/tools"),
      web_root: root.join(".local/gateway-web/site"),
      data_dir: absolute(
        root,
        options
          .data_dir
          .as_deref()
          .unwrap_or(Path::new(".local/gateway-web/data")),
      ),
      config: options.config.as_ref().map(|path| absolute(root, path)),
      listen: options.listen,
      release: options.release,
    })
  }
  pub fn server_command(&self, executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
      .current_dir(&self.root)
      .env("LYONBROWN_GATEWAY_WEB_ROOT", &self.web_root)
      .arg("--data-dir")
      .arg(&self.data_dir);
    if let Some(config) = &self.config {
      command.arg("--config").arg(config);
    }
    if let Some(listen) = &self.listen {
      command.arg("--listen").arg(listen.to_string());
    }
    command
  }
  pub fn print(&self) {
    eprintln!("rustup target list --installed (prepare wasm32-unknown-unknown if missing)");
    eprintln!(
      "cargo install --locked --root {} cargo-leptos --version {} (if missing)",
      self.tools_root.display(),
      CARGO_LEPTOS_VERSION
    );
    eprintln!(
      "{} leptos build --project gateway{}",
      self
        .tools_root
        .join("bin")
        .join(format!("cargo-leptos{}", std::env::consts::EXE_SUFFIX))
        .display(),
      if self.release { " --release" } else { "" }
    );
    eprintln!(
      "Locate ai-gateway with Cargo compiler-artifact JSON; preserve Cargo target overrides."
    );
    eprintln!(
      "Web assets: {} (workspace gateway metadata is authoritative)",
      self.web_root.display()
    );
    eprintln!("Gateway data: {}", self.data_dir.display());
    eprintln!(
      "Run the Cargo-reported ai-gateway executable with this WebUI root and the selected runtime configuration."
    );
  }
  pub async fn build(&mut self, environment: &BTreeMap<OsString, OsString>) -> Result<PathBuf> {
    let toolchain = toolchain::prepare_wasm(&self.root, environment).await?;
    let leptos = tools::prepare_leptos(self, &toolchain, environment).await?;
    build::build(self, &toolchain, &leptos, environment).await
  }
  pub async fn serve(&self, executable: &Path) -> Result<()> {
    process::serve(self.server_command(executable)).await
  }
}
fn absolute(root: &Path, path: &Path) -> PathBuf {
  if path.is_absolute() {
    path.to_owned()
  } else {
    root.join(path)
  }
}

pub fn repository_root() -> Result<PathBuf> {
  Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .canonicalize()
    .context("locate this checkout's repository root")
}
