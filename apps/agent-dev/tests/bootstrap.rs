//! Process-level failure tests. Successful real-product startup is covered by runtime acceptance.
#![cfg(unix)]
use agent_dev::{DevelopmentDaemon, Options, Plan, Target};
use anyhow::{Context, Result};
use std::{
  collections::BTreeMap, fs, net::TcpListener, os::unix::fs::PermissionsExt, time::Duration,
};

fn fixture(root: &std::path::Path, script: &str) -> Result<(Plan, std::path::PathBuf)> {
  let socket = TcpListener::bind("127.0.0.1:0")?.local_addr()?;
  let options = Options {
    target: Target::Daemon,
    data_dir: None,
    config: None,
    listen: socket,
    dry_run: false,
    native_computer: false,
  };
  let mut plan = Plan::new(root, &options)?;
  plan.prepare(&BTreeMap::new())?;
  let binary = root.join("daemon fixture");
  fs::write(&binary, format!("#!/bin/sh\n{script}\n"))?;
  fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))?;
  Ok((plan, binary))
}
#[tokio::test]
async fn foreign_listener_is_not_claimed_or_terminated() -> Result<()> {
  let root = tempfile::tempdir()?;
  let (plan, binary) = fixture(root.path(), "exit 0")?;
  let listener = TcpListener::bind(plan.listen)?;
  let result = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &BTreeMap::new(),
    Duration::from_secs(2),
  )
  .await;
  assert!(result.is_err());
  assert!(TcpListener::bind(plan.listen).is_err());
  assert!(!plan.data_dir.join("dev-instance.json").exists());
  assert!(!plan.data_dir.join("auth.token").exists());
  drop(listener);
  Ok(())
}
#[tokio::test]
async fn failed_daemon_never_publishes_a_reusable_instance() -> Result<()> {
  let root = tempfile::tempdir()?;
  let (plan, binary) = fixture(root.path(), "exit 7")?;
  let result = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &BTreeMap::new(),
    Duration::from_secs(2),
  )
  .await;
  assert!(result.is_err());
  assert!(!plan.data_dir.join("dev-instance.json").exists());
  assert_eq!(
    fs::metadata(plan.data_dir.join("auth.token"))?
      .permissions()
      .mode()
      & 0o077,
    0
  );
  assert!(TcpListener::bind(plan.listen).is_ok());
  Ok(())
}
#[tokio::test]
async fn timeout_kills_and_reaps_the_owned_process_group() -> Result<()> {
  let root = tempfile::tempdir()?;
  let (plan, binary) = fixture(root.path(), "sleep 60 &\necho $! > descendant.pid\nwait")?;
  // Include real OS process startup in this fixture's readiness budget.
  let result = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &BTreeMap::new(),
    Duration::from_secs(5),
  )
  .await;
  let error = result.err().expect("owned readiness must time out");
  assert!(error.to_string().contains("readiness timed out"));
  let pid = fs::read_to_string(root.path().join("descendant.pid"))
    .context("read timed-out daemon fixture descendant marker")?
    .trim()
    .parse::<u32>()
    .context("parse timed-out daemon fixture descendant PID")?;
  wait_dead(pid).await?;
  assert!(!plan.data_dir.join("dev-instance.json").exists());
  Ok(())
}
#[tokio::test]
async fn cancellation_drops_only_the_owned_process_group() -> Result<()> {
  let root = tempfile::tempdir()?;
  let (plan, binary) = fixture(root.path(), "sleep 60 &\necho $! > descendant.pid\nwait")?;
  let root_path = root.path().to_owned();
  let mut tasks = tokio::task::JoinSet::new();
  tasks.spawn(async move {
    DevelopmentDaemon::start(
      &root_path,
      &plan,
      &binary,
      &BTreeMap::new(),
      Duration::from_secs(60),
    )
    .await
  });
  let marker = root.path().join("descendant.pid");
  // Match the timeout fixture's OS startup budget; cancellation begins only
  // after the shell has published a complete descendant PID.
  let pid = tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      match fs::read_to_string(&marker) {
        Ok(contents) if contents.ends_with('\n') => {
          break contents
            .trim()
            .parse::<u32>()
            .context("parse cancellation fixture descendant PID");
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => break Err(error).context("read cancellation fixture descendant marker"),
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .context("owned daemon fixture did not publish its descendant PID before cancellation")??;
  tasks.abort_all();
  let error = tasks
    .join_next()
    .await
    .context("missing cancellation fixture bootstrap task")?
    .err()
    .context("daemon bootstrap completed before cancellation")?;
  assert!(error.is_cancelled(), "bootstrap task must be cancelled");
  wait_dead(pid).await?;
  Ok(())
}

async fn wait_dead(pid: u32) -> Result<()> {
  let pid = pid.to_string();
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let status = std::process::Command::new("kill")
        .args(["-0", &pid])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("inspect terminated daemon fixture descendant")?;
      if !status.success() {
        break Ok::<(), anyhow::Error>(());
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .with_context(|| {
    format!("daemon fixture descendant {pid} still exists after process-group cleanup")
  })??;
  Ok(())
}
