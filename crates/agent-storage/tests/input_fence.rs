use agent_core::{
  RunStatus,
  events::{AGENT_EVENT_KIND, AgentEvent, QueueKind, QueueStatus},
};
use agent_storage::Store;

async fn queue(store: &Store, run: &str, entry: &str, status: QueueStatus) -> anyhow::Result<()> {
  store
    .append_event_owned(
      run,
      1,
      AGENT_EVENT_KIND,
      serde_json::to_value(AgentEvent::QueueUpdated {
        entry_id: entry.into(),
        kind: QueueKind::FollowUp,
        status,
        prompt: None,
        request_id: None,
      })?,
    )
    .await?;
  Ok(())
}

#[tokio::test]
async fn late_cancellation_fences_terminal_results_and_preserves_original_request_identity()
-> anyhow::Result<()> {
  for status in [
    RunStatus::Completed,
    RunStatus::Failed,
    RunStatus::Cancelled,
  ] {
    let store = Store::open(":memory:").await?;
    let session = store
      .create_session("late cancellation", "/tmp", "local")
      .await?;
    let (run, _) = store
      .accept_run(&session.id, 1, "original prompt", "original-key")
      .await?;
    store
      .commit_snapshot(
        &session.id,
        1,
        b"durable harness state",
        "snapshot",
        serde_json::json!({}),
      )
      .await?;
    let terminal = store
      .update_run_owned(&run.id, 1, status, "preserved original result", "")
      .await?;
    queue(
      &store,
      &run.id,
      "queued-entry",
      QueueStatus::CancelRequested,
    )
    .await?;
    assert!(
      store
        .accept_run(&session.id, 1, "new prompt", "new-key")
        .await
        .is_err()
    );
    assert_eq!(
      store
        .accept_run(&session.id, 1, "original prompt", "original-key")
        .await?,
      (terminal, false)
    );
    let events = store.events(&run.id, 0, 100).await?;
    let outbox = store.pending_outbox(100).await?;
    assert!(store.fence_run_input_failure(&run.id, 2).await.is_err());
    assert_eq!(store.events(&run.id, 0, 100).await?, events);
    assert_eq!(store.pending_outbox(100).await?, outbox);
    let fenced = store.fence_run_input_failure(&run.id, 1).await?;
    assert_eq!(fenced.status, "uncertain");
    assert_eq!(fenced.result, "preserved original result");
    assert_eq!(store.run(&run.id).await?, Some(fenced.clone()));
    assert_eq!(
      store.load_snapshot(&session.id).await?,
      Some(b"durable harness state".to_vec())
    );
    let events = store.events(&run.id, 0, 100).await?;
    assert_eq!(events.last().unwrap().kind, "run_uncertain");
    let payload: serde_json::Value = serde_json::from_str(&events.last().unwrap().payload_json)?;
    assert_eq!(payload["result"], "preserved original result");
    assert_eq!(store.pending_outbox(100).await?, events);
    assert_eq!(store.fence_run_input_failure(&run.id, 1).await?, fenced);
    assert!(store.fence_run_input_failure(&run.id, 2).await.is_err());
    assert_eq!(store.events(&run.id, 0, 100).await?, events);
    assert_eq!(store.pending_outbox(100).await?, events);
    assert!(
      store
        .accept_run(&session.id, 1, "new prompt", "new-key")
        .await
        .is_err()
    );
  }
  Ok(())
}

#[tokio::test]
async fn only_durable_consumption_or_cancellation_resolves_the_host_intent() -> anyhow::Result<()> {
  for resolved in [QueueStatus::Cancelled, QueueStatus::Consumed] {
    let store = Store::open(":memory:").await?;
    let session = store.create_session("resolved", "/tmp", "local").await?;
    let (first, _) = store
      .accept_run(&session.id, 1, "original", "original")
      .await?;
    store
      .update_run_owned(&first.id, 1, RunStatus::Completed, "done", "")
      .await?;
    queue(&store, &first.id, "entry", QueueStatus::CancelRequested).await?;
    queue(&store, &first.id, "entry", QueueStatus::Pending).await?;
    assert!(
      store
        .accept_run(&session.id, 1, "next", "next")
        .await
        .is_err()
    );
    queue(&store, &first.id, "entry", resolved).await?;
    assert_eq!(store.recover_input_cancellations().await?, 0);
    let (next, created) = store.accept_run(&session.id, 1, "next", "next").await?;
    assert!(created);
    assert_ne!(next.id, first.id);
    assert_eq!(store.run(&first.id).await?.unwrap().status, "completed");
  }
  Ok(())
}

#[tokio::test]
async fn reopening_restores_an_uncertain_fence_without_replaying_or_rewriting_snapshot()
-> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("input-intent.sqlite");
  let store = Store::open(&path).await?;
  let session = store.create_session("restart", "/tmp", "local").await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "original", "original")
    .await?;
  store
    .commit_snapshot(
      &session.id,
      1,
      b"pending queue remains unchanged",
      "snapshot",
      serde_json::json!({}),
    )
    .await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Completed, "completed result", "")
    .await?;
  queue(&store, &run.id, "entry", QueueStatus::CancelRequested).await?;
  drop(store);
  let store = Store::open(&path).await?;
  assert_eq!(store.recover_input_cancellations().await?, 1);
  let fenced = store.run(&run.id).await?.unwrap();
  assert_eq!(fenced.status, "uncertain");
  assert_eq!(fenced.result, "completed result");
  assert_eq!(
    store.load_snapshot(&session.id).await?,
    Some(b"pending queue remains unchanged".to_vec())
  );
  assert!(
    store
      .accept_run(&session.id, 1, "blind replay", "another-key")
      .await
      .is_err()
  );
  let events = store.events(&run.id, 0, 100).await?;
  assert_eq!(store.pending_outbox(100).await?, events);
  drop(store);
  let store = Store::open(&path).await?;
  assert_eq!(store.recover_input_cancellations().await?, 0);
  assert_eq!(store.events(&run.id, 0, 100).await?, events);
  assert_eq!(store.pending_outbox(100).await?, events);
  assert_eq!(store.runs(&session.id).await?.len(), 1);
  Ok(())
}

#[tokio::test]
async fn input_fence_closes_pending_approval_in_the_same_ledger_transaction() -> anyhow::Result<()>
{
  let store = Store::open(":memory:").await?;
  let session = store.create_session("approval", "/tmp", "local").await?;
  let (run, _) = store.accept_run(&session.id, 1, "write", "write").await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "partial result", "")
    .await?;
  let approval = store
    .request_approval_owned(
      &run.id,
      1,
      "write_file",
      &serde_json::json!({"path":"fixture"}),
    )
    .await?;
  let fenced = store.fence_run_input_failure(&run.id, 1).await?;
  assert_eq!(fenced.status, "uncertain");
  assert_eq!(fenced.result, "partial result");
  assert_eq!(
    store.approval(&approval.id).await?.unwrap().status,
    "cancelled"
  );
  let events = store.events(&run.id, 0, 100).await?;
  assert_eq!(events[events.len() - 2].kind, "approval_closed");
  assert_eq!(events[events.len() - 1].kind, "run_uncertain");
  assert_eq!(store.pending_outbox(100).await?, events);
  Ok(())
}

#[tokio::test]
async fn malformed_known_events_fail_closed_and_unknown_events_do_not_invent_intents()
-> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let session = store
    .create_session("event contract", "/tmp", "local")
    .await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "original", "original")
    .await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Completed, "", "")
    .await?;
  store
    .append_event_owned(
      &run.id,
      1,
      AGENT_EVENT_KIND,
      serde_json::json!({"type":"future_queue_fact"}),
    )
    .await?;
  assert_eq!(store.recover_input_cancellations().await?, 0);
  let (next, _) = store.accept_run(&session.id, 1, "next", "next").await?;
  store
    .update_run_owned(&next.id, 1, RunStatus::Completed, "", "")
    .await?;
  store
    .append_event_owned(
      &next.id,
      1,
      AGENT_EVENT_KIND,
      serde_json::json!({"type":"queue_updated","entry_id":"missing fields"}),
    )
    .await?;
  let before = store.pending_outbox(100).await?;
  assert!(
    store
      .accept_run(&session.id, 1, "must reject", "reject")
      .await
      .is_err()
  );
  assert!(store.recover_input_cancellations().await.is_err());
  assert_eq!(store.pending_outbox(100).await?, before);
  assert_eq!(store.runs(&session.id).await?.len(), 2);
  Ok(())
}

#[tokio::test]
async fn fencing_an_old_source_cannot_take_over_another_active_run() -> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let session = store
    .create_session("conflicting owner", "/tmp", "local")
    .await?;
  let (old, _) = store.accept_run(&session.id, 1, "old", "old").await?;
  store
    .update_run_owned(&old.id, 1, RunStatus::Completed, "old result", "")
    .await?;
  let (active, _) = store.accept_run(&session.id, 1, "active", "active").await?;
  queue(&store, &old.id, "entry", QueueStatus::CancelRequested).await?;
  let before = store.pending_outbox(100).await?;
  assert!(store.fence_run_input_failure(&old.id, 1).await.is_err());
  assert!(store.recover_input_cancellations().await.is_err());
  assert_eq!(store.run(&old.id).await?.unwrap().status, "completed");
  assert_eq!(store.run(&active.id).await?.unwrap().status, "accepted");
  assert_eq!(store.pending_outbox(100).await?, before);
  store
    .update_run_owned(
      &active.id,
      1,
      RunStatus::Uncertain,
      "",
      "other outcome unknown",
    )
    .await?;
  let before = store.pending_outbox(100).await?;
  assert_eq!(store.recover_input_cancellations().await?, 0);
  assert_eq!(store.run(&old.id).await?.unwrap().status, "completed");
  assert_eq!(store.pending_outbox(100).await?, before);
  Ok(())
}
