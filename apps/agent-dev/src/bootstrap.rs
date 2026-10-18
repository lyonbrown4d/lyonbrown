use crate::{
  Plan,
  instance::{BootstrapLease, Instance, read_token},
  process::OwnedProcess,
};
use agent_client::AgentClient;
use agent_protocol::v1::Node;
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, net::TcpListener, path::Path, time::Duration};

/// The credential intentionally has no Debug implementation and is only passed in child env.
pub struct Connection {
  token: String,
}
impl Connection {
  pub fn token(&self) -> &str {
    &self.token
  }
}

pub struct DevelopmentDaemon {
  pub connection: Connection,
  owned: Option<OwnedProcess>,
}
impl DevelopmentDaemon {
  /// Serialize startup per canonical development directory; release the lease before client launch.
  pub async fn start(
    root: &Path,
    plan: &Plan,
    binary: &Path,
    ambient: &BTreeMap<String, String>,
    timeout: Duration,
  ) -> Result<Self> {
    let deadline = tokio::time::Instant::now() + timeout;
    let _lease = loop {
      if let Some(lease) = BootstrapLease::try_acquire(plan)? {
        break lease;
      }
      ensure!(
        tokio::time::Instant::now() < deadline,
        "timed out waiting for development bootstrap lock"
      );
      tokio::select! {
        _ = tokio::signal::ctrl_c() => anyhow::bail!("development bootstrap cancelled"),
        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
      }
    };
    let mut expected = Instance::expected(plan, binary, ambient, String::new())?;
    match TcpListener::bind(plan.listen) {
      Ok(reservation) => drop(reservation),
      Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
        let record = expected.verify_record(plan)?;
        let token = read_token(plan)?;
        probe(plan, &record, &token).await
          .context("listener failed development authentication/compatibility; stop it from its owning terminal or choose another --listen and --data-dir")?;
        return Ok(Self {
          connection: Connection { token },
          owned: None,
        });
      }
      Err(error) => return Err(error).context("check development endpoint availability"),
    }
    // A closed endpoint permits restart even when a previous instance record remains.
    if plan.data_dir.join("auth.token").try_exists()? {
      read_token(plan)?;
    } else {
      agent_security::token(&plan.data_dir)?;
    }
    let token = read_token(plan)?;
    let mut owned = OwnedProcess::spawn(binary, root, plan, ambient)?;
    expected.incarnation = owned.incarnation().to_owned();
    let ready = async {
      // This proof comes from our captured child pipe only, after that child bound/recovered.
      // Authentication alone cannot prove ownership when a same-directory daemon wins a race.
      tokio::select! {
        _ = tokio::signal::ctrl_c() => anyhow::bail!("development bootstrap cancelled"),
        _ = tokio::time::sleep_until(deadline) => anyhow::bail!("owned development agentd readiness timed out; inspect development logs"),
        result = owned.wait_ready() => result?,
      }
      wait_ready(plan, &expected, &token, &mut owned, deadline).await
    }.await;
    if let Err(error) = ready {
      owned
        .stop()
        .await
        .context("clean up failed development daemon")?;
      return Err(error);
    }
    // Refuse a configuration or artifact that changed while the daemon was starting.
    if Instance::expected(plan, binary, ambient, expected.incarnation.clone())? != expected {
      owned.stop().await?;
      anyhow::bail!("development binary or configuration changed during startup; rerun cargo dev");
    }
    owned.ensure_running()?;
    expected.publish(plan)?;
    Ok(Self {
      connection: Connection { token },
      owned: Some(owned),
    })
  }
  pub fn is_owner(&self) -> bool {
    self.owned.is_some()
  }
  pub async fn stop(&mut self) -> Result<()> {
    if let Some(owned) = &mut self.owned {
      owned.stop().await?;
    }
    Ok(())
  }
  pub async fn wait(&mut self) -> Result<()> {
    if let Some(owned) = &mut self.owned {
      owned.wait().await
    } else {
      std::future::pending().await
    }
  }
}
async fn wait_ready(
  plan: &Plan,
  expected: &Instance,
  token: &str,
  owned: &mut OwnedProcess,
  deadline: tokio::time::Instant,
) -> Result<()> {
  loop {
    owned.ensure_running()?;
    ensure!(
      tokio::time::Instant::now() < deadline,
      "development agentd readiness timed out; inspect development data/logs"
    );
    let ready = tokio::select! {
      _ = tokio::signal::ctrl_c() => anyhow::bail!("development bootstrap cancelled"),
      _ = tokio::time::sleep_until(deadline) => anyhow::bail!("development agentd readiness timed out; inspect development data/logs"),
      ready = probe(plan, expected, token) => ready,
    };
    owned.ensure_running()?;
    if ready.is_ok() {
      return Ok(());
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
  }
}
async fn probe(plan: &Plan, expected: &Instance, token: &str) -> Result<()> {
  tokio::time::timeout(Duration::from_secs(5), async {
    let client = AgentClient::connect(&plan.endpoint(), token).await?;
    let (node, incarnation) = client.development_node().await?;
    ensure!(
      incarnation.as_deref() == Some(expected.incarnation.as_str()),
      "listener is not the recorded development process incarnation; stop it from its owning terminal or choose another --listen and --data-dir"
    );
    verify_node(&node, expected)
  })
  .await
  .context("development authentication probe timed out")?
}
fn verify_node(node: &Node, expected: &Instance) -> Result<()> {
  ensure!(
    node.protocol_min <= 1 && node.protocol_max >= 1,
    "incompatible development daemon protocol"
  );
  ensure!(
    node.id == expected.node_id && node.endpoint == expected.endpoint,
    "listener identity does not match the selected development directory and endpoint"
  );
  ensure!(
    node.daemon_version == env!("CARGO_PKG_VERSION"),
    "incompatible development daemon version"
  );
  for capability in ["sessions", "runs", "tasks.manage"] {
    ensure!(
      node.capabilities.iter().any(|value| value == capability),
      "development daemon lacks required capability {capability}"
    );
  }
  ensure!(!node.draining, "development daemon is draining");
  Ok(())
}
