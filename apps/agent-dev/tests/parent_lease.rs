//! The private parent pipe must stop a real daemon even without an OS signal.
mod support;
use agent_client::AgentClient;
use agent_dev::DevelopmentDaemon;
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use support::{binary, plan};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test]
#[ignore = "requires separately built actual agentd; see support module"]
async fn losing_the_parent_pipe_stops_daemon_and_preserves_durable_session() -> Result<()> {
  let root = tempfile::tempdir()?;
  let plan = plan(root.path())?;
  let binary = binary()?;
  let ambient = BTreeMap::new();
  let nonce = uuid::Uuid::new_v4().simple().to_string();
  let mut child = tokio::process::Command::new(&binary)
    .env_clear()
    .envs(plan.environment(&ambient))
    .env("LYONBROWN_DEV_READY_NONCE", &nonce)
    .current_dir(root.path())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()?;
  let stdout = child.stdout.take().context("readiness pipe")?;
  let mut output = BufReader::new(stdout);
  let mut line = String::new();
  tokio::time::timeout(Duration::from_secs(60), output.read_line(&mut line)).await??;
  assert_eq!(line, format!("lyonbrown-dev-ready:{nonce}\n"));
  let token = std::fs::read_to_string(plan.data_dir.join("auth.token"))?;
  let client = tokio::time::timeout(Duration::from_secs(10), async {
    loop {
      ensure!(
        child.try_wait()?.is_none(),
        "owned daemon exited before authenticated readiness"
      );
      if let Ok(client) = AgentClient::connect(&plan.endpoint(), &token).await {
        break Ok::<_, anyhow::Error>(client);
      }
      tokio::time::sleep(Duration::from_millis(50)).await;
    }
  })
  .await??;
  let (node, incarnation) = client.development_node().await?;
  assert_eq!(incarnation.as_deref(), Some(nonce.as_str()));
  let session = client
    .create_session(
      "parent lifetime",
      root.path().to_str().context("fixture path")?,
    )
    .await?;
  // Closing the only writer simulates supervisor exit; send no process signal.
  drop(child.stdin.take());
  let status = tokio::time::timeout(Duration::from_secs(25), child.wait()).await??;
  assert!(
    status.success(),
    "EOF must use the daemon's normal shutdown path"
  );
  let mut restarted = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &ambient,
    Duration::from_secs(60),
  )
  .await?;
  let restored = AgentClient::connect(&plan.endpoint(), restarted.connection.token()).await?;
  assert_eq!(restored.node().await?.id, node.id);
  assert!(
    restored
      .list_sessions()
      .await?
      .iter()
      .any(|value| value.id == session.id)
  );
  restarted.stop().await?;
  Ok(())
}
