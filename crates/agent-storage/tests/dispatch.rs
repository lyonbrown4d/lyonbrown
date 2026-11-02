use agent_core::{RunStatus, TaskEnvelope, new_id};
use agent_storage::Store;
use anyhow::Result;

#[tokio::test]
async fn master_intent_fences_parent_lifecycle_session_and_paginates_stable_ids() -> Result<()> {
  let dir = tempfile::tempdir()?;
  let store = Store::open(dir.path().join("state.db")).await?;
  let session = store
    .create_session("master", "/workspace", "master")
    .await?;
  let parent = store
    .accept_run(&session.id, 1, "delegate work", "parent")
    .await?
    .0;
  let envelope = TaskEnvelope {
    task_id: new_id(),
    parent_run_id: parent.id.clone(),
    session_id: session.id.clone(),
    owner_node_id: "master".into(),
    owner_epoch: 1,
    attempt_id: new_id(),
    instructions: "inspect".into(),
    capabilities: vec!["workspace.read".into()],
    workspace: session.workspace.clone(),
    idempotency_key: new_id(),
    deadline_ms: agent_core::now_ms() + 60_000,
  };
  assert!(
    store
      .accept_master_dispatched_task(envelope.clone(), "worker")
      .await
      .is_err()
  );
  assert!(store.task(&envelope.task_id).await?.is_none());
  store
    .update_run_owned(&parent.id, 1, RunStatus::Running, "", "")
    .await?;
  let mut wrong = envelope.clone();
  wrong.workspace = "/another".into();
  assert!(
    store
      .accept_master_dispatched_task(wrong, "worker")
      .await
      .is_err()
  );
  let other = store.create_session("other", "/other", "master").await?;
  let other_run = store.accept_run(&other.id, 1, "other", "other").await?.0;
  let mut wrong = envelope.clone();
  wrong.parent_run_id = other_run.id;
  assert!(
    store
      .accept_master_dispatched_task(wrong, "worker")
      .await
      .is_err()
  );
  assert!(
    store
      .accept_master_dispatched_task(envelope.clone(), "worker")
      .await?
      .1
  );
  let mut next = envelope.clone();
  next.task_id = new_id();
  next.idempotency_key = new_id();
  next.attempt_id = new_id();
  assert!(
    store
      .accept_master_dispatched_task(next.clone(), "worker")
      .await?
      .1
  );
  assert_eq!(store.active_master_tasks("master").await?, 2);
  assert_eq!(store.active_master_tasks("another-master").await?, 0);
  store
    .finish_attempt(&envelope.task_id, &envelope.attempt_id, 1, "result", "")
    .await?;
  assert_eq!(store.active_master_tasks("master").await?, 1);
  store
    .mark_task_uncertain(&next.task_id, &next.attempt_id, 1)
    .await?;
  assert_eq!(store.active_master_tasks("master").await?, 1);
  let first = store.tasks_for_session(&session.id, "", 1).await?;
  let second = store
    .tasks_for_session(&session.id, &first[0].envelope.task_id, 1)
    .await?;
  assert_eq!(first.len(), 1);
  assert_eq!(second.len(), 1);
  assert!(first[0].envelope.task_id < second[0].envelope.task_id);
  assert!(store.tasks_for_session(&other.id, "", 1).await?.is_empty());
  store
    .update_run_owned(&parent.id, 1, RunStatus::CancelRequested, "", "")
    .await?;
  let mut late = next;
  late.task_id = new_id();
  late.idempotency_key = new_id();
  assert!(
    store
      .accept_master_dispatched_task(late, "worker")
      .await
      .is_err()
  );
  let before = store.tasks_for_session(&session.id, "", 500).await?.len();
  assert_eq!(before, 2);
  assert!(store.tasks_for_session(&session.id, "", 0).await.is_err());
  Ok(())
}
