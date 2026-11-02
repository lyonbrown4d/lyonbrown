use agent_core::{Run, RunStatus};
use agent_storage::{OperationStatus, Store};
use sea_orm::prelude::*;
use serde_json::json;

async fn running(store: &Store, key: &str) -> anyhow::Result<Run> {
  let session = store.create_session(key, "/fixture", "local").await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "perform operation", key)
    .await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await
}

#[tokio::test]
async fn restart_marks_intent_uncertain_and_forever_rejects_replay() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "restart").await?;
  store
    .begin_operation(
      &run.id,
      1,
      "action",
      "execute_command",
      json!({"argument_count":2}),
    )
    .await?;
  drop(store);
  let store = Store::open(&path).await?;
  assert_eq!(
    store.operation("action").await?.expect("intent").status,
    OperationStatus::Intent
  );
  assert!(
    store
      .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
      .await
      .is_err()
  );
  assert_eq!(store.recover_operations().await?, 1);
  let receipt = store.operation("action").await?.expect("receipt");
  assert_eq!(receipt.status, OperationStatus::Uncertain);
  assert_eq!(receipt.run_id, run.id);
  assert_eq!(receipt.intent_summary, json!({"argument_count":2}));
  assert_eq!(
    receipt.result_summary,
    Some(json!({"reason":"daemon_restarted"}))
  );
  assert!(receipt.finished_at_ms.is_some());
  assert_eq!(store.recover_operations().await?, 0);
  assert!(
    store
      .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
      .await
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 1, "action", OperationStatus::Completed, json!({}))
      .await
      .is_err()
  );
  assert_eq!(store.operation("action").await?, Some(receipt));
  let events = store.events(&run.id, 0, 100).await?;
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
  assert_eq!(store.pending_outbox(100).await?, events);
  Ok(())
}

#[tokio::test]
async fn owner_run_and_lifecycle_fences_preserve_actual_cancelled_receipt() -> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let run = running(&store, "fences").await?;
  let other = running(&store, "other").await?;
  assert!(
    store
      .finish_operation(&run.id, 1, "missing", OperationStatus::Failed, json!({}))
      .await
      .is_err()
  );
  assert!(
    store
      .begin_operation(&run.id, 2, "stale_begin", "execute_command", json!({}))
      .await
      .is_err()
  );
  store
    .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
    .await?;
  assert!(
    store
      .begin_operation(&other.id, 1, "action", "execute_command", json!({}))
      .await
      .is_err()
  );
  let before = store.events(&run.id, 0, 100).await?;
  assert!(
    store
      .finish_operation(
        &other.id,
        1,
        "action",
        OperationStatus::Completed,
        json!({})
      )
      .await
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 2, "action", OperationStatus::Completed, json!({}))
      .await
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 1, "action", OperationStatus::Intent, json!({}))
      .await
      .is_err()
  );
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  store
    .update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")
    .await?;
  assert!(
    store
      .begin_operation(&run.id, 1, "after_cancel", "execute_command", json!({}))
      .await
      .is_err()
  );
  store
    .update_run_owned(&run.id, 1, RunStatus::Cancelled, "", "")
    .await?;
  store
    .finish_operation(
      &run.id,
      1,
      "action",
      OperationStatus::Completed,
      json!({"exit_code":0}),
    )
    .await?;
  assert_eq!(
    store.operation("action").await?.expect("receipt").status,
    OperationStatus::Completed
  );
  assert_eq!(store.run(&run.id).await?.expect("run").status, "cancelled");
  assert!(
    store
      .begin_operation(&run.id, 1, "after_terminal", "execute_command", json!({}))
      .await
      .is_err()
  );
  assert!(
    store
      .finish_operation(&run.id, 1, "action", OperationStatus::Failed, json!({}))
      .await
      .is_err()
  );
  store
    .begin_operation(&other.id, 1, "new_owner", "execute_command", json!({}))
    .await?;
  store
    .change_owner(&other.session_id, 1, "replacement")
    .await?;
  assert!(
    store
      .finish_operation(
        &other.id,
        1,
        "new_owner",
        OperationStatus::Completed,
        json!({})
      )
      .await
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
      .await
      .is_err()
  );
  assert_eq!(
    store.operation("new_owner").await?.expect("intent").status,
    OperationStatus::Intent
  );
  assert!(store.operation("missing").await?.is_none());
  Ok(())
}

#[tokio::test]
async fn accepted_runs_and_raw_inputs_cannot_be_operation_intents() -> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let session = store
    .create_session("accepted", "/fixture", "local")
    .await?;
  let (run, _) = store.accept_run(&session.id, 1, "wait", "accepted").await?;
  assert!(
    store
      .begin_operation(&run.id, 1, "accepted", "execute_command", json!({}))
      .await
      .is_err()
  );
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await?;
  let before = store.events(&run.id, 0, 100).await?;
  for summary in [
    json!({"arguments":["secret"]}),
    json!({"reason":"raw command --token=secret"}),
    json!({"reason":"a".repeat(4097)}),
  ] {
    assert!(
      store
        .begin_operation(&run.id, 1, "raw", "execute_command", summary)
        .await
        .is_err()
    );
  }
  assert!(store.operation("raw").await?.is_none());
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  assert_eq!(store.pending_outbox(100).await?, before);
  Ok(())
}

#[tokio::test]
async fn concurrent_connections_commit_one_intent_only() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "race").await?;
  let left = Store::open(&path).await?;
  let right = Store::open(&path).await?;
  let id = &run.id;
  let (first, second) = tokio::join!(
    async {
      left
        .begin_operation(id, 1, "race", "execute_command", json!({}))
        .await
    },
    async {
      right
        .begin_operation(id, 1, "race", "execute_command", json!({}))
        .await
    }
  );
  let results = [first, second];
  assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
  assert_eq!(
    store.operation("race").await?.expect("intent").status,
    OperationStatus::Intent
  );
  let events = store.events(&run.id, 0, 100).await?;
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "operation_intent")
      .count(),
    1
  );
  assert_eq!(store.pending_outbox(100).await?, events);
  Ok(())
}

#[tokio::test]
async fn outbox_failure_rolls_back_intent_receipt_and_recovery() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "atomic").await?;
  let injected = store.connection();
  let before = store.events(&run.id, 0, 100).await?;
  let reject = "CREATE TRIGGER reject_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected storage fault'); END;";
  injected.execute_unprepared(reject).await?;
  assert!(
    store
      .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
      .await
      .is_err()
  );
  assert!(store.operation("action").await?.is_none());
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  assert_eq!(store.pending_outbox(100).await?, before);
  injected
    .execute_unprepared("DROP TRIGGER reject_outbox;")
    .await?;
  store
    .begin_operation(&run.id, 1, "action", "execute_command", json!({}))
    .await?;
  let before = store.events(&run.id, 0, 100).await?;
  let intent = store.operation("action").await?;
  injected.execute_unprepared(reject).await?;
  assert!(
    store
      .finish_operation(
        &run.id,
        1,
        "action",
        OperationStatus::Completed,
        json!({"exit_code":0})
      )
      .await
      .is_err()
  );
  assert!(store.recover_operations().await.is_err());
  assert_eq!(store.operation("action").await?, intent);
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  assert_eq!(store.pending_outbox(100).await?, before);
  injected
    .execute_unprepared("DROP TRIGGER reject_outbox;")
    .await?;
  store
    .finish_operation(
      &run.id,
      1,
      "action",
      OperationStatus::Failed,
      json!({"reason":"spawn_failed"}),
    )
    .await?;
  assert_eq!(
    store.operation("action").await?.expect("receipt").status,
    OperationStatus::Failed
  );
  assert_eq!(
    store.pending_outbox(100).await?,
    store.events(&run.id, 0, 100).await?
  );
  Ok(())
}
