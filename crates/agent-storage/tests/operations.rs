use agent_core::{Run, RunStatus};
use agent_storage::{OperationStatus, Store};
use diesel::{connection::SimpleConnection, prelude::*};
use serde_json::json;

fn running(store: &Store, key: &str) -> anyhow::Result<Run> {
  let session = store.create_session(key, "/fixture", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "perform operation", key)?;
  store.update_run_owned(&run.id, 1, RunStatus::Running, "", "")
}

#[test]
fn restart_marks_intent_uncertain_and_forever_rejects_replay() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let run = running(&store, "restart")?;
  store.begin_operation(
    &run.id,
    1,
    "action",
    "execute_command",
    json!({"argument_count":2}),
  )?;
  drop(store);
  let store = Store::open(&path)?;
  assert_eq!(
    store.operation("action")?.expect("intent").status,
    OperationStatus::Intent
  );
  assert!(
    store
      .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
      .is_err()
  );
  assert_eq!(store.recover_operations()?, 1);
  let receipt = store.operation("action")?.expect("receipt");
  assert_eq!(receipt.status, OperationStatus::Uncertain);
  assert_eq!(receipt.run_id, run.id);
  assert_eq!(receipt.intent_summary, json!({"argument_count":2}));
  assert_eq!(
    receipt.result_summary,
    Some(json!({"reason":"daemon_restarted"}))
  );
  assert!(receipt.finished_at_ms.is_some());
  assert_eq!(store.recover_operations()?, 0);
  assert!(
    store
      .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 1, "action", OperationStatus::Completed, json!({}))
      .is_err()
  );
  assert_eq!(store.operation("action")?, Some(receipt));
  let events = store.events(&run.id, 0, 100)?;
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "operation_intent")
      .count(),
    1
  );
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "operation_finished")
      .count(),
    1
  );
  assert_eq!(store.pending_outbox(100)?, events);
  Ok(())
}

#[test]
fn owner_run_and_lifecycle_fences_preserve_actual_cancelled_receipt() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let run = running(&store, "fences")?;
  let other = running(&store, "other")?;
  assert!(
    store
      .finish_operation(&run.id, 1, "missing", OperationStatus::Failed, json!({}))
      .is_err()
  );
  assert!(
    store
      .begin_operation(&run.id, 2, "stale_begin", "execute_command", json!({}))
      .is_err()
  );
  store.begin_operation(&run.id, 1, "action", "execute_command", json!({}))?;
  assert!(
    store
      .begin_operation(&other.id, 1, "action", "execute_command", json!({}))
      .is_err()
  );
  let before = store.events(&run.id, 0, 100)?;
  assert!(
    store
      .finish_operation(
        &other.id,
        1,
        "action",
        OperationStatus::Completed,
        json!({})
      )
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 2, "action", OperationStatus::Completed, json!({}))
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 1, "action", OperationStatus::Intent, json!({}))
      .is_err()
  );
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  store.update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")?;
  assert!(
    store
      .begin_operation(&run.id, 1, "after_cancel", "execute_command", json!({}))
      .is_err()
  );
  store.update_run_owned(&run.id, 1, RunStatus::Cancelled, "", "")?;
  store.finish_operation(
    &run.id,
    1,
    "action",
    OperationStatus::Completed,
    json!({"exit_code":0}),
  )?;
  assert_eq!(
    store.operation("action")?.expect("receipt").status,
    OperationStatus::Completed
  );
  assert_eq!(store.run(&run.id)?.expect("run").status, "cancelled");
  assert!(
    store
      .begin_operation(&run.id, 1, "after_terminal", "execute_command", json!({}))
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 1, "action", OperationStatus::Failed, json!({}))
      .is_err()
  );
  store.begin_operation(&other.id, 1, "new_owner", "execute_command", json!({}))?;
  store.change_owner(&other.session_id, 1, "replacement")?;
  assert!(
    store
      .finish_operation(
        &other.id,
        1,
        "new_owner",
        OperationStatus::Completed,
        json!({})
      )
      .is_err()
  );
  assert!(
    store
      .finish_operation(
        &other.id,
        2,
        "new_owner",
        OperationStatus::Completed,
        json!({})
      )
      .is_err()
  );
  assert_eq!(
    store.operation("new_owner")?.expect("intent").status,
    OperationStatus::Intent
  );
  assert!(store.operation("missing")?.is_none());
  Ok(())
}

#[test]
fn accepted_runs_and_raw_inputs_cannot_be_operation_intents() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let session = store.create_session("accepted", "/fixture", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "wait", "accepted")?;
  assert!(
    store
      .begin_operation(&run.id, 1, "accepted", "execute_command", json!({}))
      .is_err()
  );
  store.update_run_owned(&run.id, 1, RunStatus::Running, "", "")?;
  let before = store.events(&run.id, 0, 100)?;
  for summary in [
    json!({"arguments":["secret"]}),
    json!({"reason":"raw command --token=secret"}),
    json!({"reason":"a".repeat(4097)}),
  ] {
    assert!(
      store
        .begin_operation(&run.id, 1, "raw", "execute_command", summary)
        .is_err()
    );
  }
  assert!(store.operation("raw")?.is_none());
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  assert_eq!(store.pending_outbox(100)?, before);
  Ok(())
}

#[test]
fn concurrent_connections_commit_one_intent_only() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let run = running(&store, "race")?;
  let left = Store::open(&path)?;
  let right = Store::open(&path)?;
  let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
  let results = std::thread::scope(|scope| {
    let first_barrier = barrier.clone();
    let id = &run.id;
    let first = scope.spawn(move || {
      first_barrier.wait();
      left.begin_operation(id, 1, "race", "execute_command", json!({}))
    });
    let second = scope.spawn(move || {
      barrier.wait();
      right.begin_operation(id, 1, "race", "execute_command", json!({}))
    });
    [
      first.join().expect("first writer"),
      second.join().expect("second writer"),
    ]
  });
  assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
  assert_eq!(
    store.operation("race")?.expect("intent").status,
    OperationStatus::Intent
  );
  let events = store.events(&run.id, 0, 100)?;
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "operation_intent")
      .count(),
    1
  );
  assert_eq!(store.pending_outbox(100)?, events);
  Ok(())
}

#[test]
fn outbox_failure_rolls_back_intent_receipt_and_recovery() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let run = running(&store, "atomic")?;
  let mut injected = SqliteConnection::establish(path.to_str().expect("path"))?;
  let before = store.events(&run.id, 0, 100)?;
  let reject = "CREATE TRIGGER reject_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected storage fault'); END;";
  injected.batch_execute(reject)?;
  assert!(
    store
      .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
      .is_err()
  );
  assert!(store.operation("action")?.is_none());
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  assert_eq!(store.pending_outbox(100)?, before);
  injected.batch_execute("DROP TRIGGER reject_outbox;")?;
  store.begin_operation(&run.id, 1, "action", "execute_command", json!({}))?;
  let before = store.events(&run.id, 0, 100)?;
  let intent = store.operation("action")?;
  injected.batch_execute(reject)?;
  assert!(
    store
      .finish_operation(
        &run.id,
        1,
        "action",
        OperationStatus::Completed,
        json!({"exit_code":0})
      )
      .is_err()
  );
  assert!(store.recover_operations().is_err());
  assert_eq!(store.operation("action")?, intent);
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  assert_eq!(store.pending_outbox(100)?, before);
  injected.batch_execute("DROP TRIGGER reject_outbox;")?;
  store.finish_operation(
    &run.id,
    1,
    "action",
    OperationStatus::Failed,
    json!({"reason":"spawn_failed"}),
  )?;
  assert_eq!(
    store.operation("action")?.expect("receipt").status,
    OperationStatus::Failed
  );
  assert_eq!(store.pending_outbox(100)?, store.events(&run.id, 0, 100)?);
  Ok(())
}
