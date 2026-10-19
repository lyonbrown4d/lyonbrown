//! Foreground development supervisor, isolated from the native user service.
mod bootstrap;
mod build;
pub mod gateway;
mod instance;
mod process;

pub use bootstrap::{Connection, DevelopmentDaemon};
pub use build::{ProductBuild, build_binary, client_command};

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use std::{
  collections::BTreeMap,
  net::SocketAddr,
  path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Target {
  Daemon,
  Desktop,
  Tui,
}

#[derive(Parser)]
#[command(about = "Start or reuse an isolated development daemon, then launch a client")]
pub struct Options {
  pub target: Target,
  /// Shared development data; relative paths resolve from the repository root.
  #[arg(long)]
  pub data_dir: Option<PathBuf>,
  /// Configuration TOML file; .env is read from its directory.
  #[arg(long)]
  pub config: Option<PathBuf>,
  /// Non-production loopback socket shared by daemon and clients.
  #[arg(long, default_value = "127.0.0.1:7547")]
  pub listen: SocketAddr,
  /// Build the embedded native Computer host into the client; daemon tools still require trusted configuration.
  #[arg(long)]
  pub native_computer: bool,
  /// Print paths and build commands without creating files or reading credentials.
  #[arg(long)]
  pub dry_run: bool,
}

#[derive(Clone)]
pub struct Plan {
  pub target: Target,
  pub native_computer: bool,
  pub data_dir: PathBuf,
  pub listen: SocketAddr,
  pub config: Option<PathBuf>,
}
impl Plan {
  pub fn new(root: &Path, options: &Options) -> Result<Self> {
    ensure!(root.is_absolute(), "repository root must be absolute");
    ensure!(
      !options.native_computer || options.target != Target::Daemon,
      "--native-computer requires desktop or tui; it cannot be used with daemon"
    );
    ensure!(
      options.listen.ip().is_loopback()
        && options.listen.port() != 0
        && options.listen.port() != 7447,
      "development listen must use a nonzero loopback port other than production port 7447"
    );
    let data_dir = absolute(
      root,
      options
        .data_dir
        .as_deref()
        .unwrap_or(Path::new(".local/dev/agentd")),
    );
    let config = options.config.as_ref().map(|path| absolute(root, path));
    if let Some(path) = &config {
      ensure!(path.is_file(), "explicit configuration file is missing");
    }
    Ok(Self {
      target: options.target,
      native_computer: options.native_computer,
      data_dir,
      listen: options.listen,
      config,
    })
  }
  pub fn endpoint(&self) -> String {
    format!("http://{}", self.listen)
  }
  pub fn product_build(&self) -> ProductBuild {
    match self.target {
      Target::Daemon => ProductBuild::Daemon,
      Target::Desktop => ProductBuild::Desktop {
        native_computer: self.native_computer,
      },
      Target::Tui => ProductBuild::Tui {
        native_computer: self.native_computer,
      },
    }
  }
  pub fn package(&self) -> &'static str {
    self.product_build().package()
  }
  pub fn environment(&self, ambient: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut environment = ambient.clone();
    for name in [
      "LYONBROWN_TOKEN",
      "LYONBROWN_DEV_READY_NONCE",
      "LYONBROWN_ENDPOINT",
      "LYONBROWN_CONFIG",
      "LYONBROWN_DATA_DIR",
      "LYONBROWN_LISTEN",
    ] {
      environment.remove(name);
    }
    environment.insert("LYONBROWN_LISTEN".into(), self.listen.to_string());
    environment.insert("LYONBROWN_ENDPOINT".into(), self.endpoint());
    environment.insert(
      "LYONBROWN_DATA_DIR".into(),
      self.data_dir.to_string_lossy().into_owned(),
    );
    if let Some(config) = &self.config {
      environment.insert(
        "LYONBROWN_CONFIG".into(),
        config.to_string_lossy().into_owned(),
      );
    }
    environment
  }
  pub fn prepare(&mut self, ambient: &BTreeMap<String, String>) -> Result<()> {
    agent_config::load_with_environment(
      &self.data_dir,
      self.config.as_deref(),
      &self.environment(ambient),
    )?;
    std::fs::create_dir_all(&self.data_dir).context("create development data directory")?;
    self.data_dir = self.data_dir.canonicalize()?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&self.data_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
  }
}
fn absolute(root: &Path, path: &Path) -> PathBuf {
  if path.is_absolute() {
    path.to_owned()
  } else {
    root.join(path)
  }
}
