//! Actual daemon concurrency, persistence and compatibility acceptance.
mod support;
use agent_client::AgentClient;
use agent_dev::{DevelopmentDaemon, client_command};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, ffi::OsStr, time::Duration};
use support::{binary, plan};

#[tokio::test]
#[ignore = "requires separately built actual agentd; see module instructions"]
async fn concurrent_clients_share_authenticated_daemon_and_client_exit_preserves_it() -> Result<()>
{
  let root = tempfile::tempdir()?;
  let plan = plan(root.path())?;
  let binary = binary()?;
  let ambient = BTreeMap::from([
    ("LYONBROWN_TOKEN".into(), "production-secret".into()),
    (
      "LYONBROWN_ENDPOINT".into(),
      "https://production.invalid".into(),
    ),
  ]);
  let (first, second) = tokio::join!(
    DevelopmentDaemon::start(
      root.path(),
      &plan,
      &binary,
      &ambient,
      Duration::from_secs(60)
    ),
    DevelopmentDaemon::start(
      root.path(),
      &plan,
      &binary,
      &ambient,
      Duration::from_secs(60)
    ),
  );
  let first = first?;
  let second = second?;
  assert_ne!(
    first.is_owner(),
    second.is_owner(),
    "exactly one concurrent launcher must own agentd"
  );
  let (mut owner, reused) = if first.is_owner() {
    (first, second)
  } else {
    (second, first)
  };
  assert_eq!(owner.connection.token(), reused.connection.token());
  let token = std::fs::read_to_string(plan.data_dir.join("auth.token"))?;
  assert_eq!(owner.connection.token(), token.trim());
  let client = AgentClient::connect(&plan.endpoint(), owner.connection.token()).await?;
  let node = client.node().await?;
  let other = AgentClient::connect(&plan.endpoint(), reused.connection.token()).await?;
  assert_eq!(other.node().await?.id, node.id);
  let session = client
    .create_session(
      "shared development session",
      root.path().to_str().context("fixture path")?,
    )
    .await?;
  assert!(
    other
      .list_sessions()
      .await?
      .iter()
      .any(|value| value.id == session.id)
  );
  let mut command = client_command(
    &std::env::current_exe()?,
    root.path(),
    &plan,
    &ambient,
    &reused.connection,
  );
  let environment: BTreeMap<_, _> = command.get_envs().collect();
  assert_eq!(
    environment[OsStr::new("LYONBROWN_TOKEN")],
    Some(OsStr::new(token.trim()))
  );
  assert_eq!(
    environment[OsStr::new("LYONBROWN_ENDPOINT")],
    Some(OsStr::new(&plan.endpoint()))
  );
  assert!(
    command
      .get_args()
      .all(|arg| !arg.to_string_lossy().contains(token.trim()))
  );
  assert!(command.arg("--list").output()?.status.success());
  drop(reused);
  assert_eq!(
    client.node().await?.id,
    node.id,
    "closing a client launcher must preserve agentd"
  );
  assert!(
    client
      .list_sessions()
      .await?
      .iter()
      .any(|value| value.id == session.id)
  );
  let record = std::fs::read_to_string(plan.data_dir.join("dev-instance.json"))?;
  assert!(!record.contains(token.trim()));
  owner.stop().await?;
  assert!(
    AgentClient::connect(&plan.endpoint(), &token)
      .await
      .is_err()
  );
  // Closed endpoints permit a restart with the previous record/token and stable node identity.
  let mut restarted = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &ambient,
    Duration::from_secs(60),
  )
  .await?;
  assert!(restarted.is_owner());
  assert_eq!(
    AgentClient::connect(&plan.endpoint(), restarted.connection.token())
      .await?
      .node()
      .await?
      .id,
    node.id
  );
  let restored = AgentClient::connect(&plan.endpoint(), restarted.connection.token()).await?;
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
#[tokio::test]
#[ignore = "requires separately built actual agentd; see module instructions"]
async fn live_daemon_rejects_wrong_token_changed_configuration_and_changed_binary() -> Result<()> {
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
  let client = AgentClient::connect(&plan.endpoint(), &token).await?;
  let id = client.node().await?.id;
  std::fs::write(
    plan.data_dir.join("auth.token"),
    "wrong-development-token-0000000000000000",
  )?;
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
  std::fs::write(plan.data_dir.join("auth.token"), &token)?;
  std::fs::write(
    plan.data_dir.join("config.toml"),
    "model = 'changed-model'\n",
  )?;
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
  std::fs::remove_file(plan.data_dir.join("config.toml"))?;
  let changed_binary = root.path().join("changed-binary");
  std::fs::write(&changed_binary, "different built artifact")?;
  assert!(
    DevelopmentDaemon::start(
      root.path(),
      &plan,
      &changed_binary,
      &ambient,
      Duration::from_secs(10)
    )
    .await
    .is_err()
  );
  assert_eq!(
    client.node().await?.id,
    id,
    "rejected reuse must leave the existing daemon alive"
  );
  owner.stop().await?;
  Ok(())
}
