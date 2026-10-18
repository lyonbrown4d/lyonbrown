//! A stale record must not authorize a separately launched daemon incarnation.
mod support;
use agent_client::AgentClient;
use agent_dev::DevelopmentDaemon;
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, time::Duration};
use support::{binary, plan};

#[tokio::test]
#[ignore = "requires separately built actual agentd; see module instructions"]
async fn stale_record_cannot_adopt_a_raw_daemon_with_the_same_identity_and_token() -> Result<()> {
  let root = tempfile::tempdir()?;
  let plan = plan(root.path())?;
  let binary = binary()?;
  let ambient = BTreeMap::new();
  let mut owner = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &ambient,
    Duration::from_secs(60),
  )
  .await?;
  let token = owner.connection.token().to_owned();
  let node_id = AgentClient::connect(&plan.endpoint(), &token)
    .await?
    .node()
    .await?
    .id;
  owner.stop().await?;
  assert!(plan.data_dir.join("dev-instance.json").is_file());
  let mut raw = tokio::process::Command::new(&binary)
    .env_clear()
    .envs(plan.environment(&ambient))
    .current_dir(root.path())
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true)
    .spawn()?;
  let client = tokio::time::timeout(Duration::from_secs(60), async {
    loop {
      ensure!(
        raw.try_wait()?.is_none(),
        "raw test daemon exited before readiness"
      );
      if let Ok(client) = AgentClient::connect(&plan.endpoint(), &token).await {
        break Ok::<_, anyhow::Error>(client);
      }
      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  })
  .await??;
  let (node, incarnation) = client.development_node().await?;
  assert_eq!(node.id, node_id);
  assert!(incarnation.is_none());
  assert!(
    DevelopmentDaemon::start(
      root.path(),
      &plan,
      &binary,
      &ambient,
      Duration::from_secs(10)
    )
    .await
    .is_err()
  );
  assert_eq!(
    client.node().await?.id,
    node_id,
    "rejected reuse must preserve the unrelated raw daemon"
  );
  raw.kill().await?;
  Ok(())
}
