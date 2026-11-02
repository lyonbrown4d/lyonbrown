use agent_core::{
  RunStatus,
  events::{AGENT_EVENT_KIND, AgentEvent, QueueKind, QueueStatus},
};
use agent_storage::Store;

fn queue(store: &Store, run: &str, entry: &str, status: QueueStatus) -> anyhow::Result<()> {
  store.append_event_owned(
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
  )?;
  Ok(())
}

#[test]
fn late_cancellation_fences_terminal_results_and_preserves_original_request_identity()
-> anyhow::Result<()> {
  for status in [
    RunStatus::Completed,
    RunStatus::Failed,
    RunStatus::Cancelled,
  ] {
    let store = Store::open(":memory:")?;
    let session = store.create_session("late cancellation", "/tmp", "local")?;
    let (run, _) = store.accept_run(&session.id, 1, "original prompt", "original-key")?;
    store.commit_snapshot(
      &session.id,
      1,
      b"durable harness state",
      "snapshot",
      serde_json::json!({}),
    )?;
    let terminal = store.update_run_owned(&run.id, 1, status, "preserved original result", "")?;
    queue(
      &store,
      &run.id,
      "queued-entry",
      QueueStatus::CancelRequested,
    )?;
    assert!(
      store
        .accept_run(&session.id, 1, "new prompt", "new-key")
        .is_err()
    );
    assert_eq!(
      store.accept_run(&session.id, 1, "original prompt", "original-key")?,
      (terminal, false)
    );
    let events = store.events(&run.id, 0, 100)?;
    let outbox = store.pending_outbox(100)?;
    assert!(store.fence_run_input_failure(&run.id, 2).is_err());
    assert_eq!(store.events(&run.id, 0, 100)?, events);
    assert_eq!(store.pending_outbox(100)?, outbox);
    let fenced = store.fence_run_input_failure(&run.id, 1)?;
    assert_eq!(fenced.status, "uncertain");
    assert_eq!(fenced.result, "preserved original result");
    assert_eq!(store.run(&run.id)?, Some(fenced.clone()));
    assert_eq!(
      store.load_snapshot(&session.id)?,
      Some(b"durable harness state".to_vec())
    );
    let events = store.events(&run.id, 0, 100)?;
    assert_eq!(events.last().unwrap().kind, "run_uncertain");
    let payload: serde_json::Value = serde_json::from_str(&events.last().unwrap().payload_json)?;
    assert_eq!(payload["result"], "preserved original result");
    assert_eq!(store.pending_outbox(100)?, events);
    assert_eq!(store.fence_run_input_failure(&run.id, 1)?, fenced);
    assert!(store.fence_run_input_failure(&run.id, 2).is_err());
    assert_eq!(store.events(&run.id, 0, 100)?, events);
    assert_eq!(store.pending_outbox(100)?, events);
    assert!(
      store
        .accept_run(&session.id, 1, "new prompt", "new-key")
        .is_err()
    );
  }
  Ok(())
}

#[test]
fn only_durable_consumption_or_cancellation_resolves_the_host_intent() -> anyhow::Result<()> {
  for resolved in [QueueStatus::Cancelled, QueueStatus::Consumed] {
    let store = Store::open(":memory:")?;
    let session = store.create_session("resolved", "/tmp", "local")?;
    let (first, _) = store.accept_run(&session.id, 1, "original", "original")?;
    store.update_run_owned(&first.id, 1, RunStatus::Completed, "done", "")?;
    queue(&store, &first.id, "entry", QueueStatus::CancelRequested)?;
    queue(&store, &first.id, "entry", QueueStatus::Pending)?;
    assert!(store.accept_run(&session.id, 1, "next", "next").is_err());
    queue(&store, &first.id, "entry", resolved)?;
    assert_eq!(store.recover_input_cancellations()?, 0);
    let (next, created) = store.accept_run(&session.id, 1, "next", "next")?;
    assert!(created);
    assert_ne!(next.id, first.id);
    assert_eq!(store.run(&first.id)?.unwrap().status, "completed");
  }
  Ok(())
}

#[test]
fn reopening_restores_an_uncertain_fence_without_replaying_or_rewriting_snapshot()
-> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("input-intent.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("restart", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "original", "original")?;
  store.commit_snapshot(
    &session.id,
    1,
    b"pending queue remains unchanged",
    "snapshot",
    serde_json::json!({}),
  )?;
  store.update_run_owned(&run.id, 1, RunStatus::Completed, "completed result", "")?;
  queue(&store, &run.id, "entry", QueueStatus::CancelRequested)?;
  drop(store);
  let store = Store::open(&path)?;
  assert_eq!(store.recover_input_cancellations()?, 1);
  let fenced = store.run(&run.id)?.unwrap();
  assert_eq!(fenced.status, "uncertain");
  assert_eq!(fenced.result, "completed result");
  assert_eq!(
    store.load_snapshot(&session.id)?,
    Some(b"pending queue remains unchanged".to_vec())
  );
  assert!(
    store
      .accept_run(&session.id, 1, "blind replay", "another-key")
      .is_err()
  );
  let events = store.events(&run.id, 0, 100)?;
  assert_eq!(store.pending_outbox(100)?, events);
  drop(store);
  let store = Store::open(&path)?;
  assert_eq!(store.recover_input_cancellations()?, 0);
  assert_eq!(store.events(&run.id, 0, 100)?, events);
  assert_eq!(store.pending_outbox(100)?, events);
  assert_eq!(store.runs(&session.id)?.len(), 1);
  Ok(())
}

#[test]
fn input_fence_closes_pending_approval_in_the_same_ledger_transaction() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let session = store.create_session("approval", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "write", "write")?;
  store.update_run_owned(&run.id, 1, RunStatus::Running, "partial result", "")?;
  let approval = store.request_approval_owned(
    &run.id,
    1,
    "write_file",
    &serde_json::json!({"path":"fixture"}),
  )?;
  let fenced = store.fence_run_input_failure(&run.id, 1)?;
  assert_eq!(fenced.status, "uncertain");
  assert_eq!(fenced.result, "partial result");
  assert_eq!(store.approval(&approval.id)?.unwrap().status, "cancelled");
  let events = store.events(&run.id, 0, 100)?;
  assert_eq!(events[events.len() - 2].kind, "approval_closed");
  assert_eq!(events[events.len() - 1].kind, "run_uncertain");
  assert_eq!(store.pending_outbox(100)?, events);
  Ok(())
}

#[test]
fn malformed_known_events_fail_closed_and_unknown_events_do_not_invent_intents()
-> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let session = store.create_session("event contract", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "original", "original")?;
  store.update_run_owned(&run.id, 1, RunStatus::Completed, "", "")?;
  store.append_event_owned(
    &run.id,
    1,
    AGENT_EVENT_KIND,
    serde_json::json!({"type":"future_queue_fact"}),
  )?;
  assert_eq!(store.recover_input_cancellations()?, 0);
  let (next, _) = store.accept_run(&session.id, 1, "next", "next")?;
  store.update_run_owned(&next.id, 1, RunStatus::Completed, "", "")?;
  store.append_event_owned(
    &next.id,
    1,
    AGENT_EVENT_KIND,
    serde_json::json!({"type":"queue_updated","entry_id":"missing fields"}),
  )?;
  let before = store.pending_outbox(100)?;
  assert!(
    store
      .accept_run(&session.id, 1, "must reject", "reject")
      .is_err()
  );
  assert!(store.recover_input_cancellations().is_err());
  assert_eq!(store.pending_outbox(100)?, before);
  assert_eq!(store.runs(&session.id)?.len(), 2);
  Ok(())
}

#[test]
fn fencing_an_old_source_cannot_take_over_another_active_run() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let session = store.create_session("conflicting owner", "/tmp", "local")?;
  let (old, _) = store.accept_run(&session.id, 1, "old", "old")?;
  store.update_run_owned(&old.id, 1, RunStatus::Completed, "old result", "")?;
  let (active, _) = store.accept_run(&session.id, 1, "active", "active")?;
  queue(&store, &old.id, "entry", QueueStatus::CancelRequested)?;
  let before = store.pending_outbox(100)?;
  assert!(store.fence_run_input_failure(&old.id, 1).is_err());
  assert!(store.recover_input_cancellations().is_err());
  assert_eq!(store.run(&old.id)?.unwrap().status, "completed");
  assert_eq!(store.run(&active.id)?.unwrap().status, "accepted");
  assert_eq!(store.pending_outbox(100)?, before);
  store.update_run_owned(
    &active.id,
    1,
    RunStatus::Uncertain,
    "",
    "other outcome unknown",
  )?;
  let before = store.pending_outbox(100)?;
  assert_eq!(store.recover_input_cancellations()?, 0);
  assert_eq!(store.run(&old.id)?.unwrap().status, "completed");
  assert_eq!(store.pending_outbox(100)?, before);
  Ok(())
}
