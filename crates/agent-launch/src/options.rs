use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, ensure};
use directories::ProjectDirs;
use serde::Deserialize;

use crate::model::{Registration, Requirements};

#[derive(Clone, Debug)]
pub struct LaunchOptions {
  pub bundled_agentd: PathBuf,
  pub data_dir: PathBuf,
  pub requirements: Requirements,
}

impl LaunchOptions {
  pub fn bundled(capabilities: impl IntoIterator<Item = impl Into<String>>) -> Result<Self> {
    let executable = std::env::current_exe().context("locate client executable")?;
    let parent = executable
      .parent()
      .context("client executable has no parent directory")?;
    let name = if cfg!(windows) {
      "agentd.exe"
    } else {
      "agentd"
    };
    let sibling = parent.join(name);
    let agentd = if sibling.is_file() {
      sibling
    } else if cfg!(target_os = "macos") {
      PathBuf::from("/Library/Application Support/Lyonbrown/agentd")
    } else if cfg!(target_os = "windows") {
      windows_daemon_path()?
    } else {
      PathBuf::from("/usr/bin/agentd")
    };
    let data_dir = default_data_dir()?;
    Ok(Self {
      bundled_agentd: agentd,
      data_dir,
      requirements: Requirements {
        protocol: 1,
        capabilities: capabilities.into_iter().map(Into::into).collect(),
      },
    })
  }

  pub(crate) async fn candidate(&self) -> Result<Registration> {
    ensure!(
      self.data_dir.is_absolute(),
      "daemon data directory must be absolute"
    );
    ensure!(
      self.bundled_agentd.is_file(),
      "bundled agentd is missing: {}; install the complete client package",
      self.bundled_agentd.display()
    );
    let output = tokio::time::timeout(
      Duration::from_secs(3),
      tokio::process::Command::new(&self.bundled_agentd)
        .arg("--compatibility-json")
        .kill_on_drop(true)
        .output(),
    )
    .await
    .context("bundled agentd compatibility check timed out")??;
    ensure!(
      output.status.success(),
      "bundled agentd compatibility check failed"
    );
    #[derive(Deserialize)]
    struct Compatibility {
      daemon_version: semver::Version,
      protocol_min: u32,
      protocol_max: u32,
      storage_schema_min: u32,
      storage_schema_max: u32,
    }
    let info: Compatibility = serde_json::from_slice(&output.stdout)
      .context("invalid bundled agentd compatibility response")?;
    ensure!(
      info.protocol_min <= self.requirements.protocol
        && self.requirements.protocol <= info.protocol_max,
      "bundled agentd cannot serve the client's protocol"
    );
    ensure!(
      info.storage_schema_min <= info.storage_schema_max,
      "invalid bundled storage schema range"
    );
    Ok(Registration {
      executable: self.bundled_agentd.canonicalize()?,
      data_dir: self.data_dir.clone(),
      version: info.daemon_version,
      storage_schema_min: info.storage_schema_min,
      storage_schema_max: info.storage_schema_max,
    })
  }
}

pub(crate) fn default_data_dir() -> Result<PathBuf> {
  Ok(
    ProjectDirs::from("org", "Lyonbrown", "Lyonbrown")
      .context("locate daemon data directory")?
      .data_local_dir()
      .to_path_buf(),
  )
}

fn windows_daemon_path() -> Result<PathBuf> {
  #[cfg(windows)]
  {
    Ok(
      crate::native::windows::daemon_directory()?
        .join(env!("CARGO_PKG_VERSION"))
        .join("agentd.exe"),
    )
  }
  #[cfg(not(windows))]
  {
    anyhow::bail!("Windows native service path is unavailable on this platform")
  }
}
