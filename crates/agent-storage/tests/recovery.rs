use agent_core::RunStatus;
use agent_storage::{RecoverRun, RecoveryAction, Store};
use diesel::{connection::SimpleConnection, prelude::*};

fn request(id: &str, epoch: u64) -> RecoverRun<'_> {
  RecoverRun {
    run_id: id,
    owner_epoch: epoch,
    node_id: "local",
    action: RecoveryAction::ContinueNewSession,
    idempotency_key: "recovery-key",
    note: "Verified external effects; do not repeat the interrupted command",
    prompt: "Only explain the verified result",
  }
}

#[test]
fn recovery_cannot_orphan_running_or_unadopted_worker_tasks() -> anyhow::Result<()> {
  for succeeded in [false, true] {
    let store = Store::open(":memory:")?;
    let session = store.create_session("original", "/tmp", "local")?;
    let (run, _) = store.accept_run(&session.id, 1, "delegate", "original")?;
    store.update_run_owned(&run.id, 1, RunStatus::Running, "", "")?;
    let (task, _) = store.accept_master_dispatched_task(
      agent_core::TaskEnvelope {
        task_id: agent_core::new_id(),
        attempt_id: agent_core::new_id(),
        parent_run_id: run.id.clone(),
        session_id: session.id.clone(),
        owner_node_id: "local".into(),
        owner_epoch: 1,
        instructions: "read only".into(),
        capabilities: vec!["workspace.read".into()],
        workspace: "/tmp".into(),
        idempotency_key: "worker-key".into(),
        deadline_ms: agent_core::now_ms() + 60_000,
      },
      "worker",
    )?;
    let id = &task.envelope.task_id;
    let attempt = &task.envelope.attempt_id;
    if succeeded {
      store.finish_attempt(id, attempt, 1, "worker result", "")?;
    }
    store.update_run_owned(&run.id, 1, RunStatus::Uncertain, "", "unknown")?;
    assert!(store.recover_run(request(&run.id, 1)).is_err());
    assert!(store.run_recovery(&run.id)?.is_none());
    assert_eq!(store.session(&session.id)?.unwrap().owner_epoch, 1);
    if succeeded {
      store.accept_result(id, attempt, 1, false)?;
    } else {
      store.mark_task_cancelled(id, attempt, 1)?;
    }
    assert_eq!(store.unresolved_recovery_tasks(&session.id)?, 0);
    assert!(store.recover_run(request(&run.id, 1))?.1);
    assert_eq!(store.session(&session.id)?.unwrap().owner_epoch, 2);
  }
  Ok(())
}

#[test]
fn recovery_is_durable_idempotent_and_preserves_unknown_history() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("original", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "do not replay this", "original")?;
  store.update_run_owned(&run.id, 1, RunStatus::Running, "", "")?;
  store.commit_snapshot(
    &session.id,
    1,
    b"unknown Harness history",
    "snapshot",
    serde_json::json!({}),
  )?;
  store.update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")?;
  store.recover_interrupted()?;
  let original = store.run(&run.id)?.expect("run");
  let (resolution, created) = store.recover_run(request(&run.id, 1))?;
  assert!(created);
  let fresh_id = resolution.session_id.as_deref().expect("new session");
  let replacement = store
    .run(resolution.replacement_run_id.as_deref().expect("new run"))?
    .expect("run");
  assert_eq!(replacement.prompt, "Only explain the verified result");
  assert_eq!(replacement.status, "accepted");
  assert_ne!(fresh_id, session.id);
  assert!(store.load_snapshot(fresh_id)?.is_none());
  assert_eq!(
    store.load_snapshot(&session.id)?,
    Some(b"unknown Harness history".to_vec())
  );
  assert_eq!(store.run(&run.id)?, Some(original.clone()));
  assert_eq!(store.session(&session.id)?.expect("session").owner_epoch, 2);
  assert!(
    store
      .commit_snapshot(
        &session.id,
        1,
        b"late callback",
        "snapshot",
        serde_json::json!({})
      )
      .is_err()
  );
  assert!(
    store
      .append_event_owned(&run.id, 1, "late_callback", serde_json::json!({}))
      .is_err()
  );
  assert!(
    store
      .accept_run(&session.id, 1, "blind retry", "retry")
      .is_err()
  );
  assert!(
    store
      .accept_run(&session.id, 2, "blind retry", "current-owner-retry")
      .is_err()
  );
  let events = store.events(&run.id, 0, 100)?;
  assert_eq!(
    events.last().expect("recovery event").kind,
    "run_recovery_recorded"
  );
  assert!(
    store
      .pending_outbox(100)?
      .contains(events.last().expect("event"))
  );
  drop(store);
  let restored = Store::open(&path)?;
  assert_eq!(
    restored.recover_run(request(&run.id, 1))?,
    (resolution.clone(), false)
  );
  assert_eq!(restored.run_recovery(&run.id)?, Some(resolution));
  assert_eq!(restored.events(&run.id, 0, 100)?, events);
  assert_eq!(restored.pending_runs()?, [replacement]);
  assert!(restored.recover_interrupted()?.is_empty());
  let mut changed = request(&run.id, 1);
  changed.note = "different decision";
  assert!(restored.recover_run(changed).is_err());
  let mut changed = request(&run.id, 1);
  changed.prompt = "repeat the original side effect";
  assert!(restored.recover_run(changed).is_err());
  restored.change_owner(&session.id, 2, "local")?;
  assert!(restored.recover_run(request(&run.id, 1)).is_err());
  assert_eq!(restored.run(&run.id)?, Some(original));
  Ok(())
}

#[test]
fn recovery_outbox_failure_rolls_back_new_session_run_and_resolution() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("original", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "unknown", "original")?;
  store.update_run_owned(&run.id, 1, RunStatus::Uncertain, "", "unknown effect")?;
  let before = store.events(&run.id, 0, 100)?;
  let outbox = store.pending_outbox(100)?;
  let mut injected = SqliteConnection::establish(path.to_str().expect("path"))?;
  injected.batch_execute("CREATE TRIGGER reject_recovery_outbox BEFORE INSERT ON outbox WHEN EXISTS (SELECT 1 FROM events WHERE run_id = NEW.run_id AND cursor = NEW.cursor AND json_extract(record, '$.kind') = 'run_recovery_recorded') BEGIN SELECT RAISE(ABORT, 'injected recovery storage fault'); END;")?;
  assert!(store.recover_run(request(&run.id, 1)).is_err());
  assert!(store.run_recovery(&run.id)?.is_none());
  assert_eq!(store.sessions()?, [session]);
  assert!(store.pending_runs()?.is_empty());
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  assert_eq!(store.pending_outbox(100)?, outbox);
  injected.batch_execute("DROP TRIGGER reject_recovery_outbox;")?;
  assert!(store.recover_run(request(&run.id, 1))?.1);
  Ok(())
}

#[test]
fn abandon_requires_human_note_local_master_and_uncertain_terminal_state() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let session = store.create_session("original", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "unknown", "original")?;
  let mut input = request(&run.id, 1);
  input.action = RecoveryAction::Abandon;
  input.prompt = "";
  assert!(store.recover_run(input).is_err());
  let original = store.update_run_owned(&run.id, 1, RunStatus::Uncertain, "", "unknown")?;
  let mut input = request(&run.id, 1);
  input.note = " ";
  assert!(store.recover_run(input).is_err());
  let mut input = request(&run.id, 1);
  input.node_id = "other";
  assert!(store.recover_run(input).is_err());
  let mut input = request(&run.id, 1);
  input.action = RecoveryAction::Abandon;
  input.prompt = "";
  let (resolution, _) = store.recover_run(input)?;
  assert!(resolution.session_id.is_none());
  assert!(resolution.replacement_run_id.is_none());
  assert_eq!(store.run(&run.id)?, Some(original));
  assert_eq!(store.sessions()?.len(), 1);
  assert!(store.pending_runs()?.is_empty());
  Ok(())
}
