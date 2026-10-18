use std::{path::PathBuf, time::Duration};

use agent_client::AgentClient;
use anyhow::{Context, Result, ensure};

use crate::model::{DaemonInfo, DaemonProbe, LOCAL_ENDPOINT};

pub struct AuthenticatedProbe {
  pub token_path: PathBuf,
}

impl AuthenticatedProbe {
  async fn client(&self) -> Result<AgentClient> {
    let token = tokio::fs::read_to_string(&self.token_path)
      .await
      .context("read daemon auth.token")?;
    tokio::time::timeout(
      Duration::from_secs(2),
      AgentClient::connect_authenticated(LOCAL_ENDPOINT, &token),
    )
    .await
    .context("daemon authentication timed out")?
  }

  async fn inspect_once(&self) -> Result<DaemonInfo> {
    let client = self.client().await?;
    let node = tokio::time::timeout(Duration::from_secs(2), client.node())
      .await
      .context("daemon handshake timed out")??;
    Ok(DaemonInfo {
      version: node
        .daemon_version
        .parse()
        .context("daemon reported an invalid version")?,
      protocol_min: node.protocol_min,
      protocol_max: node.protocol_max,
      capabilities: node.capabilities.into_iter().collect(),
      active_runs: u64::from(node.active_runs),
      storage_schema_version: node.storage_schema_version,
      draining: node.draining,
    })
  }
}

impl DaemonProbe for AuthenticatedProbe {
  async fn inspect(&self) -> Result<DaemonInfo> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
      let result = self.inspect_once().await;
      if result.is_ok() || tokio::time::Instant::now() >= deadline {
        return result;
      }
      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  }

  async fn drain(&self) -> Result<bool> {
    let client = self.client().await?;
    let node = tokio::time::timeout(Duration::from_secs(2), client.prepare_upgrade())
      .await
      .context("daemon maintenance request timed out; inspect its state before retrying")??;
    if node.active_runs != 0 {
      tokio::time::timeout(Duration::from_secs(2), client.resume_accepting())
        .await
        .context("resuming task acceptance timed out")??;
      return Ok(false);
    }
    ensure!(node.draining, "daemon did not acknowledge maintenance mode");
    Ok(true)
  }
}
