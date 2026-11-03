use agent_changes::{ChangeSet, ChangeState, PatchInput, build_preview};
use agent_core::{Run, RunStatus, new_id};
use agent_storage::{OperationStatus, Store, changes::PrepareChange};
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};

async fn setup() -> anyhow::Result<(Store, Run, ChangeSet)> {
  let store = Store::open(":memory:").await?;
  let session = store.create_session("changes", "/fixture", "local").await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "apply changes", &new_id())
    .await?;
  let run = store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await?;
  let id = new_id();
  let change = build_preview(
    id.clone(),
    run.id.clone(),
    1,
    vec![(
      PatchInput {
        path: "new.txt".into(),
        expected_revision: String::new(),
        patch: "--- original\n+++ modified\n@@ -0,0 +1 @@\n+created\n".into(),
        delete: false,
      },
      None,
    )],
  )?;
  let change = store.save_change_preview(&id, change).await?;
  Ok((store, run, change))
}
fn exact(change: &ChangeSet) -> Value {
  json!({"request_id":new_id(),"change_id":change.id,"expected_revision":change.revision,
    "preview_sha256":change.preview_sha256,"run_id":change.run_id,"owner_epoch":change.owner_epoch,"action_id":new_id()})
}
fn prepare<'a>(exact: &'a Value, approval: &'a str) -> PrepareChange<'a> {
  PrepareChange {
    request_id: exact["request_id"].as_str().expect("request"),
    change_id: exact["change_id"].as_str().expect("change"),
    expected_revision: exact["expected_revision"].as_u64().expect("revision"),
    preview_sha256: exact["preview_sha256"].as_str().expect("hash"),
    run_id: exact["run_id"].as_str().expect("run"),
    owner_epoch: exact["owner_epoch"].as_u64().expect("epoch"),
    action_id: exact["action_id"].as_str().expect("action"),
    arguments: exact,
    approval_id: approval,
  }
}
async fn approve(store: &Store, change: &ChangeSet, exact: &Value) -> anyhow::Result<String> {
  let approval = store
    .request_approval_owned(&change.run_id, change.owner_epoch, "patch_apply", exact)
    .await?;
  store.decide_approval(&approval.id, true).await?;
  Ok(approval.id)
}
#[tokio::test]
async fn intent_approval_cas_receipt_and_outbox_commit_together() -> anyhow::Result<()> {
  let (store, run, change) = setup().await?;
  let exact = exact(&change);
  let approval = approve(&store, &change, &exact).await?;
  let before = store.events(&run.id, 0, 100).await?;
  // A failed outbox write must roll back approval consumption and Prepared intent.
  store.connection().execute_unprepared("CREATE TRIGGER reject_change_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected fault'); END;").await?;
  assert!(
    store
      .prepare_change(prepare(&exact, &approval))
      .await
      .is_err()
  );
  assert_eq!(
    store.change_set(&change.id).await?.state,
    ChangeState::Preview
  );
  assert_eq!(
    store.approval(&approval).await?.expect("approval").status,
    "approved"
  );
  assert!(
    store
      .operation(exact["action_id"].as_str().expect("action"))
      .await?
      .is_none()
  );
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  store
    .connection()
    .execute_unprepared("DROP TRIGGER reject_change_outbox;")
    .await?;
  let proof = store.prepare_change(prepare(&exact, &approval)).await?;
  assert_eq!(proof.change().state, ChangeState::Prepared);
  assert!(
    store
      .prepare_change(prepare(&exact, &approval))
      .await
      .is_err()
  );
  store
    .update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")
    .await?;
  let receipt = store
    .finish_change(proof, vec!["new.txt".into()], true)
    .await?;
  assert_eq!(receipt.state, ChangeState::Applied);
  assert_eq!(receipt.revision, 3);
  assert_eq!(
    store
      .operation(exact["action_id"].as_str().expect("action"))
      .await?
      .expect("operation")
      .status,
    OperationStatus::Completed
  );
  assert_eq!(
    store.pending_outbox(100).await?,
    store.events(&run.id, 0, 100).await?
  );
  Ok(())
}
#[tokio::test]
async fn interrupted_and_partial_changes_remain_uncertain_without_replay() -> anyhow::Result<()> {
  let (store, _, change) = setup().await?;
  let exact = exact(&change);
  let approval = approve(&store, &change, &exact).await?;
  let proof = store.prepare_change(prepare(&exact, &approval)).await?;
  assert_eq!(store.recover_changes().await?, 1);
  assert_eq!(store.recover_changes().await?, 0);
  assert_eq!(
    store.change_set(&change.id).await?.state,
    ChangeState::Uncertain
  );
  assert!(store.finish_change(proof, vec![], false).await.is_err());
  assert!(
    store
      .prepare_change(prepare(&exact, &approval))
      .await
      .is_err()
  );
  assert_eq!(
    store
      .operation(exact["action_id"].as_str().expect("action"))
      .await?
      .expect("operation")
      .status,
    OperationStatus::Uncertain
  );
  let (store, _, change) = setup().await?;
  let exact = self::exact(&change);
  let approval = approve(&store, &change, &exact).await?;
  let proof = store.prepare_change(prepare(&exact, &approval)).await?;
  assert_eq!(
    store.finish_change(proof, vec![], false).await?.state,
    ChangeState::Uncertain
  );
  Ok(())
}
#[tokio::test]
async fn validation_status_comes_from_actual_command_receipt_and_scope() -> anyhow::Result<()> {
  let (store, run, change) = setup().await?;
  let early = new_id();
  store
    .begin_operation(&run.id, 1, &early, "execute_command", json!({}))
    .await?;
  let exact = exact(&change);
  let approval = approve(&store, &change, &exact).await?;
  let proof = store.prepare_change(prepare(&exact, &approval)).await?;
  let applied = store
    .finish_change(proof, vec!["new.txt".into()], true)
    .await?;
  assert!(
    store
      .record_change_validation(
        &new_id(),
        &change.id,
        &run.id,
        1,
        applied.revision,
        "model says passed"
      )
      .await
      .is_err()
  );
  store
    .finish_operation(
      &run.id,
      1,
      &early,
      OperationStatus::Completed,
      json!({"exit_code":0}),
    )
    .await?;
  assert!(
    store
      .record_change_validation(&new_id(), &change.id, &run.id, 1, applied.revision, &early)
      .await
      .is_err()
  );
  let operation = new_id();
  store
    .begin_operation(&run.id, 1, &operation, "execute_command", json!({}))
    .await?;
  assert!(
    store
      .record_change_validation(
        &new_id(),
        &change.id,
        &run.id,
        1,
        applied.revision,
        &operation
      )
      .await
      .is_err()
  );
  store
    .finish_operation(
      &run.id,
      1,
      &operation,
      OperationStatus::Failed,
      json!({"exit_code":1}),
    )
    .await?;
  let record = store
    .record_change_validation(
      &new_id(),
      &change.id,
      &run.id,
      1,
      applied.revision,
      &operation,
    )
    .await?;
  assert!(!record.passed);
  assert!(!record.verifies_current_workspace);
  assert_eq!(record.exit_code, Some(1));
  assert_eq!(record.change_sha256, applied.preview_sha256);
  assert_eq!(store.change_validations(&change.id).await?.len(), 1);
  let other = store.create_session("other", "/fixture", "local").await?;
  let (other, _) = store.accept_run(&other.id, 1, "other", &new_id()).await?;
  store
    .update_run_owned(&other.id, 1, RunStatus::Running, "", "")
    .await?;
  assert!(
    store
      .record_change_validation(
        &new_id(),
        &change.id,
        &other.id,
        1,
        applied.revision,
        &operation
      )
      .await
      .is_err()
  );
  Ok(())
}
