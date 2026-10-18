use std::{collections::BTreeSet, path::PathBuf};

use anyhow::{Result, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};

pub const SERVICE_ID: &str = "org.lyonbrown.agentd";
pub const LOCAL_ENDPOINT: &str = "http://127.0.0.1:7447";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
  pub executable: PathBuf,
  pub data_dir: PathBuf,
  pub version: Version,
  pub storage_schema_min: u32,
  pub storage_schema_max: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonInfo {
  pub version: Version,
  pub protocol_min: u32,
  pub protocol_max: u32,
  pub capabilities: BTreeSet<String>,
  pub active_runs: u64,
  pub storage_schema_version: u32,
  pub draining: bool,
}

#[derive(Clone, Debug)]
pub struct Requirements {
  pub protocol: u32,
  pub capabilities: BTreeSet<String>,
}

impl Requirements {
  pub fn check(&self, daemon: &DaemonInfo) -> Result<()> {
    ensure!(
      daemon.protocol_min <= self.protocol && self.protocol <= daemon.protocol_max,
      "daemon {} supports protocol {}..={}, client requires {}",
      daemon.version,
      daemon.protocol_min,
      daemon.protocol_max,
      self.protocol
    );
    let missing: Vec<_> = self.capabilities.difference(&daemon.capabilities).collect();
    ensure!(
      missing.is_empty(),
      "daemon lacks required capabilities: {missing:?}"
    );
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceState {
  Absent,
  Stopped,
  Running,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ManagedState {
  Ready(Registration),
  Transition {
    previous: Option<Registration>,
    desired: Registration,
  },
}

/// Service operations never spawn an ordinary background daemon.
pub trait NativeService {
  fn status(&self) -> Result<ServiceState>;
  fn validate(&self, registration: &Registration) -> Result<()>;
  fn install(&self, registration: &Registration) -> Result<()>;
  /// Replace a stopped managed service. Account-based managers override this
  /// with an in-place configuration change so stored logon credentials survive.
  fn update(&self, registration: &Registration) -> Result<()> {
    self.uninstall()?;
    self.install(registration)
  }
  fn start(&self) -> Result<()>;
  fn stop(&self) -> Result<()>;
  fn uninstall(&self) -> Result<()>;
}

pub trait DaemonProbe {
  fn inspect(&self) -> impl Future<Output = Result<DaemonInfo>>;
  /// True atomically rejects subsequent submissions and confirms idle.
  /// False means active work was found and acceptance has been resumed.
  fn drain(&self) -> impl Future<Output = Result<bool>>;
}
