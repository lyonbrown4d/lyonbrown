use agent_changes::{ChangeSet, ChangeState, PatchInput, build_preview, reverse_preview};
use agent_core::{Run, RunStatus, new_id};
use agent_storage::{
  Store,
  changes::{PrepareChange, UserPrepareChange},
};
use sea_orm::ConnectionTrait;
use serde_json::json;

async fn completed() -> anyhow::Result<(Store, Run, ChangeSet)> {
  let store = Store::open(":memory:").await?;
  let session = store
    .create_session("user undo", "/fixture", "local")
    .await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "change", &new_id())
    .await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await?;
  let id = new_id();
  let change = build_preview(
    id.clone(),
    run.id.clone(),
    1,
    vec![(
      PatchInput {
        path: "created".into(),
        expected_revision: String::new(),
        patch: "--- original\n+++ modified\n@@ -0,0 +1 @@\n+content\n".into(),
        delete: false,
      },
      None,
    )],
  )?;
  let change = store.save_change_preview(&id, change).await?;
  let request = new_id();
  let action = new_id();
  let exact = json!({"request_id":request,"change_id":change.id,"expected_revision":1,"preview_sha256":change.preview_sha256,"run_id":run.id,"owner_epoch":1,"action_id":action});
  let approval = store
    .request_approval_owned(&run.id, 1, "patch_apply", &exact)
    .await?;
  store.decide_approval(&approval.id, true).await?;
  let proof = store
    .prepare_change(PrepareChange {
      request_id: &request,
      change_id: &change.id,
      expected_revision: 1,
      preview_sha256: &change.preview_sha256,
      run_id: &run.id,
      owner_epoch: 1,
      action_id: &action,
      arguments: &exact,
      approval_id: &approval.id,
    })
    .await?;
  let applied = store
    .finish_change(proof, vec!["created".into()], true)
    .await?;
  let run = store
    .update_run_owned(&run.id, 1, RunStatus::Completed, "done", "")
    .await?;
  Ok((store, run, applied))
}
fn apply<'a>(
  run: &'a Run,
  change: &'a ChangeSet,
  request: &'a str,
  action: &'a str,
) -> UserPrepareChange<'a> {
  UserPrepareChange {
    session_id: &run.session_id,
    owner_epoch: 1,
    request_id: request,
    change_id: &change.id,
    expected_revision: change.revision,
    preview_sha256: &change.preview_sha256,
    action_id: action,
  }
}
#[tokio::test]
async fn completed_run_undo_has_a_separate_exact_human_proof_without_restarting_the_run()
-> anyhow::Result<()> {
  let (store, run, applied) = completed().await?;
  let id = new_id();
  let reverse = reverse_preview(id.clone(), run.id.clone(), 1, &applied)?;
  let reverse = store
    .save_user_change_preview(&run.session_id, 1, &id, reverse)
    .await?;
  assert!(reverse.files[0].after.is_none());
  let request = new_id();
  let action = new_id();
  let proof = store
    .prepare_user_change(apply(&run, &reverse, &request, &action))
    .await?;
  assert_eq!(store.run(&run.id).await?.expect("run").status, "completed");
  assert_eq!(proof.change().state, ChangeState::Prepared);
  assert!(
    store
      .accept_run(
        &run.session_id,
        1,
        "do not race the native writer",
        &new_id()
      )
      .await
      .is_err()
  );
  assert!(
    store
      .prepare_user_change(apply(&run, &reverse, &request, &action))
      .await
      .is_err()
  );
  let undone = store
    .finish_change(proof, vec!["created".into()], true)
    .await?;
  assert_eq!(undone.state, ChangeState::Applied);
  assert_eq!(store.run(&run.id).await?.expect("run").status, "completed");
  let events = store.events(&run.id, 0, 100).await?;
  assert!(
    events
      .iter()
      .any(|event| event.kind == "user_change_authorized")
  );
  assert_eq!(store.pending_outbox(100).await?, events);
  Ok(())
}
#[tokio::test]
async fn user_approval_intent_is_atomic_and_rejects_stale_or_active_session_state()
-> anyhow::Result<()> {
  let (store, run, applied) = completed().await?;
  let id = new_id();
  let reverse = reverse_preview(id.clone(), run.id.clone(), 1, &applied)?;
  let reverse = store
    .save_user_change_preview(&run.session_id, 1, &id, reverse)
    .await?;
  let request = new_id();
  let action = new_id();
  let mut stale = apply(&run, &reverse, &request, &action);
  stale.owner_epoch = 2;
  assert!(store.prepare_user_change(stale).await.is_err());
  let mut changed = apply(&run, &reverse, &request, &action);
  changed.preview_sha256 = "wrong";
  assert!(store.prepare_user_change(changed).await.is_err());
  let before = store.events(&run.id, 0, 100).await?;
  store.connection().execute_unprepared("CREATE TRIGGER reject_user_change BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'fault'); END;").await?;
  assert!(
    store
      .prepare_user_change(apply(&run, &reverse, &request, &action))
      .await
      .is_err()
  );
  assert_eq!(
    store.change_set(&reverse.id).await?.state,
    ChangeState::Preview
  );
  assert!(store.operation(&action).await?.is_none());
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  store
    .connection()
    .execute_unprepared("DROP TRIGGER reject_user_change;")
    .await?;
  let (active, _) = store
    .accept_run(&run.session_id, 1, "another real run", &new_id())
    .await?;
  assert!(
    store
      .prepare_user_change(apply(&run, &reverse, &request, &action))
      .await
      .is_err()
  );
  store
    .update_run_owned(&active.id, 1, RunStatus::Uncertain, "", "unknown effect")
    .await?;
  assert!(
    store
      .prepare_user_change(apply(&run, &reverse, &request, &action))
      .await
      .is_err()
  );
  assert!(store.operation(&action).await?.is_none());
  Ok(())
}
