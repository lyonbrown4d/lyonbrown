//! Opt-in macOS service acceptance. Uses the real per-user launchd identity and
//! refuses to touch an existing registration. Run only on an idle test account.
#![cfg(target_os = "macos")]

use std::{collections::BTreeSet, path::PathBuf};

use agent_launch::{
  LaunchOptions, ensure_daemon,
  model::{ManagedState, NativeService, Requirements, ServiceState},
  native::NativeManager,
  state::ServiceLock,
};
use anyhow::{Context, Result, ensure};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "registers a real launchd service; requires an absent service and LYONBROWN_SERVICE_AGENTD"]
async fn native_launchd_reuses_daemon_and_restores_durable_session_after_restart() -> Result<()> {
  let bundled_agentd = PathBuf::from(
    std::env::var_os("LYONBROWN_SERVICE_AGENTD")
      .context("set LYONBROWN_SERVICE_AGENTD to the built production agentd")?,
  );
  ensure!(bundled_agentd.is_file(), "production agentd is missing");
  {
    let registry = ServiceLock::for_current_user()?;
    ensure!(
      registry.read()?.is_none() && NativeManager.status()? == ServiceState::Absent,
      "native acceptance requires an absent service and managed registration"
    );
    ensure!(
      std::net::TcpListener::bind("127.0.0.1:7447").is_ok(),
      "native acceptance requires an unused daemon port"
    );
  }
  // Preserve the directory on a failure: a live service must never lose its
  // executable or durable state merely because the test returned an error.
  let data_dir = tempfile::Builder::new()
    .prefix("lyonbrown-native-service-")
    .tempdir()?
    .keep();
  eprintln!("native service acceptance data: {}", data_dir.display());
  let options = LaunchOptions {
    bundled_agentd,
    data_dir: data_dir.clone(),
    requirements: Requirements {
      protocol: 1,
      capabilities: BTreeSet::from(["runs".into()]),
    },
  };
  let cli = ensure_daemon(options.clone()).await?;
  let cli_node = cli.node().await?;
  let session = cli
    .create_session(
      "Native service acceptance",
      data_dir.to_str().context("UTF-8 data path")?,
    )
    .await?;
  let registry = ServiceLock::for_current_user()?;
  let registration = match registry.read()? {
    Some(ManagedState::Ready(value)) => value,
    _ => anyhow::bail!("native registration did not reach Ready"),
  };
  ensure!(
    registration.data_dir == data_dir,
    "registration ownership differs"
  );
  NativeManager.validate(&registration)?;
  ensure!(NativeManager.status()? == ServiceState::Running);
  drop(registry);

  let desktop = ensure_daemon(options.clone()).await?;
  ensure!(
    desktop.node().await?.id == cli_node.id,
    "clients reached different daemons"
  );
  ensure!(
    desktop
      .list_sessions()
      .await?
      .iter()
      .any(|row| row.id == session.id)
  );

  // The same user's global service cannot be redirected to another data tree.
  let mut other_options = options.clone();
  other_options.data_dir = data_dir.join("different-client");
  let error = match ensure_daemon(other_options).await {
    Ok(_) => anyhow::bail!("another data directory unexpectedly replaced the daemon"),
    Err(error) => error,
  };
  ensure!(format!("{error:#}").contains("different data directory"));
  ensure!(cli.node().await?.id == cli_node.id);

  {
    let _registry = ServiceLock::for_current_user()?;
    NativeManager.validate(&registration)?;
    let idle = cli.prepare_upgrade().await?;
    ensure!(idle.active_runs == 0 && idle.draining, "daemon is not idle");
    NativeManager.stop()?;
    ensure!(NativeManager.status()? == ServiceState::Stopped);
  }
  drop(cli);
  drop(desktop);
  let restarted = ensure_daemon(options).await?;
  ensure!(
    restarted.node().await?.id == cli_node.id,
    "node identity changed on restart"
  );
  ensure!(
    restarted
      .list_sessions()
      .await?
      .iter()
      .any(|row| row.id == session.id)
  );
  let registry = ServiceLock::for_current_user()?;
  NativeManager.validate(&registration)?;
  let idle = restarted.prepare_upgrade().await?;
  ensure!(
    idle.active_runs == 0 && idle.draining,
    "cleanup requires an idle daemon"
  );
  NativeManager.stop()?;
  NativeManager.validate(&registration)?;
  NativeManager.uninstall()?;
  ensure!(NativeManager.status()? == ServiceState::Absent);
  registry.clear()?;
  drop(registry);
  drop(restarted);
  std::fs::remove_dir_all(&data_dir)
    .context("remove owned acceptance data after service cleanup")?;
  eprintln!("native service removed; shared registration and durable restart acceptance passed");
  Ok(())
}
