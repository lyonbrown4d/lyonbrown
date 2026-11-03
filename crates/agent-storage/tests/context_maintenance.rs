use agent_core::{
  RunStatus,
  context_maintenance::{ContextMaintenanceKind as Kind, ContextMaintenanceState as State},
};
use agent_storage::Store;
use serde_json::json;
async fn completed(
  store: &Store,
  title: &str,
) -> anyhow::Result<(agent_core::Session, agent_core::Run)> {
  let session = store.create_session(title, "/tmp", "local").await?;
  let (run, _) = store.accept_run(&session.id, 1, "fixture", title).await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await?;
  let run = store
    .update_run_owned(&run.id, 1, RunStatus::Completed, "done", "")
    .await?;
  Ok((session, run))
}
#[tokio::test]
async fn maintenance_fences_only_its_session_and_snapshot_updates_share_the_job_transaction()
-> anyhow::Result<()> {
  let data = tempfile::tempdir()?;
  let store = Store::open(&data.path().join("context.sqlite")).await?;
  let (session, run) = completed(&store, "first").await?;
  let job = store
    .begin_context_maintenance(&session.id, &run.id, 1, 4, Kind::Compact)
    .await?;
  assert!(
    store
      .accept_run(&session.id, 1, "blocked", "blocked")
      .await
      .is_err()
  );
  assert!(store.change_owner(&session.id, 1, "other").await.is_err());
  assert!(
    store
      .set_interaction_mode(
        &session.id,
        1,
        0,
        agent_core::interaction::InteractionMode::Plan
      )
      .await
      .is_err()
  );
  let other = store.create_session("independent", "/tmp", "local").await?;
  assert!(
    store
      .accept_run(&other.id, 1, "not blocked", "other")
      .await
      .is_ok()
  );
  let events = store.events(&run.id, 0, 100).await?;
  assert!(
    store
      .commit_run_snapshot_with_events(
        &session.id,
        99,
        Some(&run.id),
        b"stale",
        "context_summary_sending",
        json!({}),
        &[]
      )
      .await
      .is_err()
  );
  assert!(store.load_snapshot(&session.id).await?.is_none());
  assert_eq!(store.events(&run.id, 0, 100).await?, events);
  store
    .commit_run_snapshot_with_events(
      &session.id,
      1,
      Some(&run.id),
      b"sent",
      "context_summary_sending",
      json!({}),
      &[],
    )
    .await?;
  assert_eq!(
    store
      .context_maintenance_job(&job.id)
      .await?
      .expect("job")
      .state,
    State::Sending
  );
  store.request_context_cancellation(&job.id, 1).await?;
  store
    .commit_run_snapshot_with_events(
      &session.id,
      1,
      Some(&run.id),
      b"unknown",
      "execution_uncertain",
      json!({}),
      &[],
    )
    .await?;
  assert_eq!(
    store
      .context_maintenance_job(&job.id)
      .await?
      .expect("job")
      .state,
    State::Uncertain
  );
  assert_eq!(store.context_maintenance_count().await?, 1);
  let request = uuid::Uuid::now_v7().to_string();
  let (abandoned, owner) = store
    .abandon_context_maintenance(
      &job.id,
      1,
      &request,
      "Retire unknown context and continue in a new session",
    )
    .await?;
  assert_eq!(abandoned.state, State::Abandoned);
  assert_eq!(owner.owner_epoch, 2);
  assert_eq!(
    store.load_snapshot(&session.id).await?.as_deref(),
    Some(b"unknown".as_slice())
  );
  assert_eq!(
    store.context_maintenance_count().await?,
    0,
    "resolved abandonment permits upgrades"
  );
  assert!(
    store
      .accept_run(&session.id, 2, "must use new session", "new")
      .await
      .is_err()
  );
  assert_eq!(
    store
      .abandon_context_maintenance(
        &job.id,
        1,
        &request,
        "Retire unknown context and continue in a new session"
      )
      .await?,
    (abandoned, owner)
  );
  assert!(
    store
      .abandon_context_maintenance(
        &job.id,
        1,
        &uuid::Uuid::now_v7().to_string(),
        "different request"
      )
      .await
      .is_err()
  );
  assert_eq!(
    store
      .pending_outbox(100)
      .await?
      .into_iter()
      .filter(|event| event.run_id == run.id)
      .collect::<Vec<_>>(),
    store.events(&run.id, 0, 100).await?
  );
  Ok(())
}
#[tokio::test]
async fn cancellation_before_send_rolls_back_the_send_marker_and_restart_never_replays()
-> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let (session, run) = completed(&store, "cancel").await?;
  let job = store
    .begin_context_maintenance(&session.id, &run.id, 1, 1, Kind::Compact)
    .await?;
  store.request_context_cancellation(&job.id, 1).await?;
  let before = store.events(&run.id, 0, 100).await?;
  assert!(
    store
      .commit_run_snapshot_with_events(
        &session.id,
        1,
        Some(&run.id),
        b"must not commit",
        "context_summary_sending",
        json!({}),
        &[]
      )
      .await
      .is_err()
  );
  assert!(store.load_snapshot(&session.id).await?.is_none());
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  let (cancelled_session, cancelled_run) = completed(&store, "guard cancellation").await?;
  let cancelled = store
    .begin_context_maintenance(
      &cancelled_session.id,
      &cancelled_run.id,
      1,
      1,
      Kind::Compact,
    )
    .await?;
  store.request_context_cancellation(&cancelled.id, 1).await?;
  store
    .commit_run_snapshot_with_events(
      &cancelled_session.id,
      1,
      Some(&cancelled_run.id),
      b"cleared before send",
      "context_summary_failed",
      json!({}),
      &[],
    )
    .await?;
  assert_eq!(
    store
      .context_maintenance_job(&cancelled.id)
      .await?
      .expect("cancelled job")
      .state,
    State::Cancelled
  );
  let (other, other_run) = completed(&store, "sent").await?;
  let sent = store
    .begin_context_maintenance(&other.id, &other_run.id, 1, 1, Kind::Compact)
    .await?;
  store
    .commit_run_snapshot_with_events(
      &other.id,
      1,
      Some(&other_run.id),
      b"sent",
      "context_summary_sending",
      json!({}),
      &[],
    )
    .await?;
  let preparing = store.recover_context_maintenance().await?;
  assert_eq!(preparing.len(), 1);
  assert_eq!(preparing[0].id, job.id);
  assert_eq!(
    store
      .context_maintenance_job(&sent.id)
      .await?
      .expect("sent job")
      .state,
    State::Uncertain
  );
  store
    .finish_context_maintenance(&job.id, 1, State::Cancelled)
    .await?;
  assert!(store.recover_context_maintenance().await?.is_empty());
  assert_eq!(
    store.load_snapshot(&other.id).await?.as_deref(),
    Some(b"sent".as_slice())
  );
  assert_eq!(store.context_maintenance_count().await?, 1);
  Ok(())
}
