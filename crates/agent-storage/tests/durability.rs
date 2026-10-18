use agent_core::{RunStatus, TaskEnvelope};
use agent_storage::Store;
use diesel::{connection::SimpleConnection, prelude::*};
use serde_json::json;

#[test]
fn restart_preserves_safe_pending_and_marks_interrupted_uncertain() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("running", "/tmp", "local")?;
  let (running, _) = store.accept_run(&session.id, 1, "act", "first")?;
  store.update_run_owned(&running.id, 1, RunStatus::Running, "", "")?;
  let other = store.create_session("pending", "/tmp", "local")?;
  let (pending, _) = store.accept_run(&other.id, 1, "wait", "second")?;
  drop(store);
  let restored = Store::open(&path)?;
  assert_eq!(
    restored
      .recover_interrupted()?
      .iter()
      .map(|run| &run.id)
      .collect::<Vec<_>>(),
    [&running.id]
  );
  assert_eq!(restored.run(&running.id)?.expect("run").status, "uncertain");
  assert_eq!(restored.pending_runs()?, [pending]);
  assert!(
    restored
      .accept_run(&session.id, 1, "retry blindly", "third")
      .is_err()
  );
  assert!(restored.recover_interrupted()?.is_empty());
  Ok(())
}

#[test]
fn idempotency_owner_fencing_and_atomic_snapshot_outbox() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("test", "/tmp", "local")?;
  let (run, created) = store.accept_run(&session.id, 1, "hello", "key")?;
  assert!(created);
  assert_eq!(
    store.accept_run(&session.id, 1, "hello", "key")?,
    (run.clone(), false)
  );
  assert!(
    store
      .accept_run(&session.id, 1, "different", "key")
      .is_err()
  );
  assert!(
    store
      .accept_run(&session.id, 1, "parallel", "other")
      .is_err()
  );
  store.commit_snapshot(&session.id, 1, b"first", "snapshot", json!({"revision":1}))?;
  assert_eq!(store.load_snapshot(&session.id)?, Some(b"first".to_vec()));
  let events = store.events(&run.id, 0, 100)?;
  assert_eq!(events.len(), 2);
  assert_eq!(store.pending_outbox(100)?, events);
  let mut injected = SqliteConnection::establish(path.to_str().expect("path"))?;
  injected.batch_execute("CREATE TRIGGER reject_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected storage fault'); END;")?;
  assert!(
    store
      .commit_snapshot(&session.id, 1, b"second", "snapshot", json!({}))
      .is_err()
  );
  assert_eq!(store.load_snapshot(&session.id)?, Some(b"first".to_vec()));
  assert_eq!(store.events(&run.id, 0, 100)?, events);
  injected.batch_execute("DROP TRIGGER reject_outbox;")?;
  store.change_owner(&session.id, 1, "new-master")?;
  assert!(
    store
      .commit_snapshot(&session.id, 1, b"stale", "snapshot", json!({}))
      .is_err()
  );
  assert!(
    store
      .update_run_owned(&run.id, 1, RunStatus::Completed, "stale", "")
      .is_err()
  );
  assert_eq!(store.events(&run.id, 0, 100)?, events);
  store.acknowledge_outbox(&run.id, 1)?;
  let pending_for_run: Vec<_> = store
    .pending_outbox(100)?
    .into_iter()
    .filter(|event| event.run_id == run.id)
    .collect();
  assert_eq!(pending_for_run, events[1..]);
  Ok(())
}

#[test]
fn worker_success_requires_master_acceptance_and_current_attempt() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let envelope = TaskEnvelope {
    task_id: "task".into(),
    parent_run_id: "parent".into(),
    session_id: "session".into(),
    owner_node_id: "master".into(),
    owner_epoch: 3,
    attempt_id: "attempt".into(),
    instructions: "inspect".into(),
    capabilities: vec!["read".into()],
    workspace: "/tmp".into(),
    idempotency_key: "task-key".into(),
    deadline_ms: i64::MAX,
  };
  assert!(store.accept_task(envelope.clone())?.1);
  assert!(!store.accept_task(envelope.clone())?.1);
  let mut conflicting = envelope;
  conflicting.instructions = "different".into();
  assert!(store.accept_task(conflicting).is_err());
  assert!(
    store
      .finish_attempt("task", "stale", 3, "answer", "")
      .is_err()
  );
  let succeeded = store.finish_attempt("task", "attempt", 3, "answer", "")?;
  assert_eq!(succeeded.attempt_status, "succeeded");
  assert_eq!(succeeded.status, "awaiting_acceptance");
  assert!(store.accept_result("task", "attempt", 2, true).is_err());
  assert_eq!(
    store.accept_result("task", "attempt", 3, true)?.status,
    "completed"
  );
  assert!(store.accept_result("task", "attempt", 3, true).is_err());
  Ok(())
}

#[test]
fn concurrent_resubmission_commits_one_run_and_one_acceptance_event() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("concurrent", "/tmp", "local")?;
  let left = Store::open(&path)?;
  let right = Store::open(&path)?;
  let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
  let results = std::thread::scope(|scope| {
    let first_barrier = barrier.clone();
    let id = &session.id;
    let first = scope.spawn(move || {
      first_barrier.wait();
      left.accept_run(id, 1, "same prompt", "same-key")
    });
    let second = scope.spawn(move || {
      barrier.wait();
      right.accept_run(id, 1, "same prompt", "same-key")
    });
    [
      first.join().expect("first writer"),
      second.join().expect("second writer"),
    ]
  });
  let [left, right] = results;
  let (left, left_created) = left?;
  let (right, right_created) = right?;
  assert_eq!(left.id, right.id);
  assert_eq!(usize::from(left_created) + usize::from(right_created), 1);
  assert_eq!(store.runs(&session.id)?.len(), 1);
  assert_eq!(store.events(&left.id, 0, 100)?.len(), 1);
  assert_eq!(store.pending_outbox(100)?.len(), 1);
  Ok(())
}

#[test]
fn accepted_cancel_wins_completion_commit_and_unknown_outcomes_stay_fenced() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let session = store.create_session("cancel", "/workspace", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "act", "cancel-key")?;
  store.update_run_owned(&run.id, 1, RunStatus::Running, "", "")?;
  store.update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")?;
  let terminal = store.update_run_owned(&run.id, 1, RunStatus::Completed, "partial", "")?;
  assert_eq!(terminal.status, "cancelled");
  assert_eq!(
    store
      .events(&run.id, 0, 100)?
      .last()
      .expect("terminal event")
      .kind,
    "run_cancelled"
  );
  let (unknown, _) = store.accept_run(&session.id, 1, "external action", "unknown-key")?;
  store.update_run_owned(&unknown.id, 1, RunStatus::Running, "", "")?;
  store.update_run_owned(&unknown.id, 1, RunStatus::CancelRequested, "", "")?;
  store.update_run_owned(
    &unknown.id,
    1,
    RunStatus::Uncertain,
    "",
    "external result unknown",
  )?;
  for status in [
    RunStatus::Completed,
    RunStatus::Cancelled,
    RunStatus::CancelRequested,
  ] {
    assert!(
      store
        .update_run_owned(&unknown.id, 1, status, "", "")
        .is_err()
    );
  }
  assert_eq!(store.run(&unknown.id)?.expect("run").status, "uncertain");
  assert!(
    store
      .accept_run(&session.id, 1, "blind replay", "other-key")
      .is_err()
  );
  Ok(())
}
