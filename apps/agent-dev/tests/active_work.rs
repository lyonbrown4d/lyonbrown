//! Real-binary acceptance: client detachment must not cancel or replay daemon-owned work.
//! Set LYONBROWN_DEV_TEST_AGENTD to the separately built agentd executable.
mod support;

use agent_client::AgentClient;
use agent_dev::DevelopmentDaemon;
use agent_protocol::v1::Run;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, time::Duration};
use support::{binary, plan};

#[tokio::test]
#[ignore = "requires separately built actual agentd; see module instructions"]
async fn active_run_survives_client_detachment_and_completed_result_survives_restart() -> Result<()>
{
  let root = tempfile::tempdir()?;
  let plan = plan(root.path())?;
  let binary = binary()?;
  let protocol = StandardProtocol::Responses;
  let model = MockModelServer::start_interactive().await?;
  std::fs::write(
    plan.data_dir.join("config.toml"),
    format!(
      "provider = '{}'\nmodel = 'dev-lifecycle-fixture'\napi_base = '{}'\nrun_timeout_secs = 180\nbrowser_tools = false\n",
      protocol.provider(),
      model.base_url(),
    ),
  )?;
  let ambient = BTreeMap::from([(
    "OPENAI_API_KEY".into(),
    "synthetic-development-lifecycle-key".into(),
  )]);
  let mut owner = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &ambient,
    Duration::from_secs(60),
  )
  .await?;
  ensure!(owner.is_owner(), "first launcher must own the daemon");
  let reused = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &ambient,
    Duration::from_secs(60),
  )
  .await?;
  ensure!(!reused.is_owner(), "second launcher must reuse the daemon");
  let submitting = AgentClient::connect(&plan.endpoint(), reused.connection.token()).await?;
  let observer = AgentClient::connect(&plan.endpoint(), owner.connection.token()).await?;
  let node_id = observer.node().await?.id;
  let session = submitting
    .create_session(
      "detached client lifecycle",
      root.path().to_str().context("UTF-8 fixture workspace")?,
    )
    .await?;
  let run = submitting
    .start_run(
      &session.id,
      "Return the local lifecycle fixture response.",
      &uuid::Uuid::new_v4().to_string(),
    )
    .await?;
  tokio::time::timeout(Duration::from_secs(60), model.wait_for_requests(1))
    .await
    .context("daemon must send its first real Responses request")?;
  let active = observer.get_run(&run.id).await?;
  assert_eq!(active.status, "running");
  assert!(active.result.is_empty());
  let requests = model.requests().await;
  assert_eq!(requests.len(), 1);
  assert_eq!(requests[0].path, protocol.path());

  // Both client channels are real authenticated connections; only the submitting one closes.
  drop(submitting);
  drop(reused);
  assert!(owner.is_owner());
  assert_eq!(observer.node().await?.id, node_id);
  assert_eq!(observer.get_run(&run.id).await?.status, "running");
  assert_eq!(model.requests().await.len(), 1);

  let reply = "The daemon completed the job after its submitting client closed.";
  model.push_reply(text_reply(protocol, reply)).await;
  let completed = wait_completed(&observer, &run.id).await?;
  assert_eq!(completed.session_id, session.id);
  assert_eq!(completed.result, reply);
  assert!(completed.error.is_empty());
  assert_eq!(model.requests().await.len(), 1);
  drop(observer);
  owner.stop().await?;
  drop(owner);

  let mut restarted = DevelopmentDaemon::start(
    root.path(),
    &plan,
    &binary,
    &ambient,
    Duration::from_secs(60),
  )
  .await?;
  ensure!(
    restarted.is_owner(),
    "restart must own a newly started daemon"
  );
  let restored = AgentClient::connect(&plan.endpoint(), restarted.connection.token()).await?;
  assert_eq!(restored.node().await?.id, node_id);
  assert!(
    restored
      .list_sessions()
      .await?
      .iter()
      .any(|persisted| persisted.id == session.id)
  );
  let persisted = restored.get_run(&run.id).await?;
  assert_eq!(persisted.id, run.id);
  assert_eq!(persisted.session_id, session.id);
  assert_eq!(persisted.status, "completed");
  assert_eq!(persisted.result, reply);
  assert!(persisted.error.is_empty());
  assert_eq!(restored.list_runs(&session.id).await?.len(), 1);
  assert_eq!(
    model.requests().await.len(),
    1,
    "client detachment and restart must not replay completed inference"
  );
  drop(restored);
  restarted.stop().await?;
  Ok(())
}

async fn wait_completed(client: &AgentClient, id: &str) -> Result<Run> {
  tokio::time::timeout(Duration::from_secs(60), async {
    loop {
      let run = client.get_run(id).await?;
      if run.status == "completed" {
        return Ok(run);
      }
      ensure!(
        matches!(run.status.as_str(), "accepted" | "running"),
        "fixture run ended unexpectedly with status {}",
        run.status
      );
      tokio::time::sleep(Duration::from_millis(25)).await;
    }
  })
  .await
  .context("fixture run must complete after releasing the model response")?
}
